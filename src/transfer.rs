//! Transfer orchestration — port of the FileManager's `doUpload` / `doDownload`
//! (see `docs/spec-filemanager.md` §8). Directory transfers use tar staging;
//! a per-file SFTP fallback runs when the remote has no `tar`.

use crate::models::{FileInfo, TransferDirection, TransferProgress, TransferStatus};
use crate::remote_fs::{self, shell_quote, TransferCtx};
use crate::session::{AppEvent, SessionManager};
use anyhow::Result;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TransferJob {
    pub session_id: String,
    pub direction: TransferDirection,
    pub items: Vec<FileInfo>,
    pub target_dir: String,
    pub overwrite_all: bool,
}

fn emit_row(
    bus: &crate::session::EventBus,
    id: &str,
    name: &str,
    direction: TransferDirection,
    total: u64,
    transferred: u64,
    status: TransferStatus,
    target: &str,
    session_id: &str,
    group_id: Option<&str>,
    error: Option<String>,
) {
    bus.send(AppEvent::Transfer(Box::new(TransferProgress {
        id: id.to_string(),
        file_name: name.to_string(),
        direction,
        total,
        transferred,
        status,
        error,
        target_path: Some(target.to_string()),
        conflict_path: None,
        conflict_is_dir: false,
        session_id: Some(session_id.to_string()),
        group_id: group_id.map(|s| s.to_string()),
        created_at_ms: crate::models::now_millis(),
    })));
}

impl SessionManager {
    /// Kick off a transfer job in the background.
    pub fn start_transfer(&self, job: TransferJob) {
        let Some(session) = self.get(&job.session_id) else {
            self.bus.send(AppEvent::Toast {
                message: "会话不存在，无法传输".into(),
                kind: crate::session::ToastKind::Error,
            });
            return;
        };
        let bus = self.bus.clone();
        let cancelled = self.cancelled_handle();
        self.rt.spawn(async move {
            let group_id = if job.items.len() > 1 {
                Some(format!(
                    "batch-{}:{}:{}",
                    job.direction.as_str(),
                    job.session_id,
                    crate::models::now_millis()
                ))
            } else {
                None
            };

            if let Some(gid) = group_id.as_deref() {
                let label = match job.direction {
                    TransferDirection::Upload => format!("上传 {} 项", job.items.len()),
                    TransferDirection::Download => format!("下载 {} 项", job.items.len()),
                };
                emit_row(
                    &bus,
                    gid,
                    &label,
                    job.direction,
                    job.items.len() as u64,
                    0,
                    TransferStatus::Pending,
                    &job.target_dir,
                    &job.session_id,
                    Some(gid),
                    None,
                );
            }

            let can_tar = remote_fs::remote_has_tar(&session).await;
            let result = match job.direction {
                TransferDirection::Upload => {
                    run_upload(&session, &job, group_id.as_deref(), can_tar, &bus, &cancelled).await
                }
                TransferDirection::Download => {
                    run_download(&session, &job, group_id.as_deref(), can_tar, &bus, &cancelled).await
                }
            };

            if let Some(gid) = group_id.as_deref() {
                let (transferred, status, error) = match result {
                    Ok(count) => (
                        count,
                        if count >= job.items.len() as u64 {
                            TransferStatus::Done
                        } else {
                            TransferStatus::Error
                        },
                        if count >= job.items.len() as u64 {
                            None
                        } else {
                            Some("部分文件传输失败".to_string())
                        },
                    ),
                    Err(e) => (0, TransferStatus::Error, Some(e.to_string())),
                };
                emit_row(
                    &bus,
                    gid,
                    match job.direction {
                        TransferDirection::Upload => format!("上传 {} 项", job.items.len()),
                        TransferDirection::Download => format!("下载 {} 项", job.items.len()),
                    }
                    .as_str(),
                    job.direction,
                    job.items.len() as u64,
                    transferred,
                    status,
                    &job.target_dir,
                    &job.session_id,
                    Some(gid),
                    error,
                );
            }

            // Refresh the target panel when the transfer finishes.
            match job.direction {
                TransferDirection::Upload => {
                    bus.send(AppEvent::RemoteDir {
                        request: 0,
                        session_id: job.session_id.clone(),
                        path: job.target_dir.clone(),
                        result: Err("__refresh__".into()),
                    });
                }
                TransferDirection::Download => {
                    let _ = crate::local_fs::list_dir(&job.target_dir);
                }
            }
        });
    }

    fn cancelled_handle(&self) -> crate::session::CancelSet {
        self.cancel_set()
    }
}

fn ctx_for(
    job: &TransferJob,
    transfer_id: String,
    display_name: String,
    progress_start: u64,
    progress_span: u64,
    target: String,
    group_id: Option<&str>,
    bus: &crate::session::EventBus,
    cancelled: &crate::session::CancelSet,
) -> TransferCtx {
    TransferCtx {
        transfer_id,
        display_name,
        direction: job.direction,
        progress_total: 100,
        progress_start,
        progress_span,
        target_path: target,
        session_id: job.session_id.clone(),
        group_id: group_id.map(|s| s.to_string()),
        cancelled: cancelled.clone(),
        bus: bus.clone(),
    }
}

async fn run_upload(
    session: &Arc<crate::session::LiveSession>,
    job: &TransferJob,
    group_id: Option<&str>,
    can_tar: bool,
    bus: &crate::session::EventBus,
    cancelled: &crate::session::CancelSet,
) -> Result<u64> {
    let folders: Vec<&FileInfo> = job.items.iter().filter(|i| i.is_dir).collect();
    let files: Vec<&FileInfo> = job.items.iter().filter(|i| !i.is_dir).collect();
    let mut completed = 0u64;

    for folder in folders {
        let remote_target = remote_fs::join_path(&job.target_dir, &folder.name);
        let transfer_id = format!("upload:{remote_target}");
        emit_row(
            bus,
            &transfer_id,
            &folder.name,
            TransferDirection::Upload,
            100,
            0,
            TransferStatus::Pending,
            &remote_target,
            &job.session_id,
            group_id,
            None,
        );

        if can_tar {
            if let Err(e) = upload_folder_tar(
                session,
                job,
                folder,
                &remote_target,
                &transfer_id,
                group_id,
                bus,
                cancelled,
            )
            .await
            {
                emit_row(
                    bus,
                    &transfer_id,
                    &folder.name,
                    TransferDirection::Upload,
                    100,
                    0,
                    TransferStatus::Error,
                    &remote_target,
                    &job.session_id,
                    group_id,
                    Some(e.to_string()),
                );
            }
        } else if let Err(e) = upload_folder_files(
            session,
            job,
            folder,
            &remote_target,
            &transfer_id,
            group_id,
            bus,
            cancelled,
        )
        .await
        {
            emit_row(
                bus,
                &transfer_id,
                &folder.name,
                TransferDirection::Upload,
                100,
                0,
                TransferStatus::Error,
                &remote_target,
                &job.session_id,
                group_id,
                Some(e.to_string()),
            );
        }
        completed += 1;
        update_parent(bus, group_id, job, completed, TransferStatus::Transferring);
    }

    for file in files {
        let remote_target = remote_fs::join_path(&job.target_dir, &file.name);
        let transfer_id = format!("upload:{remote_target}");
        let ctx = ctx_for(
            job,
            transfer_id.clone(),
            file.name.clone(),
            0,
            100,
            remote_target.clone(),
            group_id,
            bus,
            cancelled,
        );
        if let Err(e) =
            remote_fs::upload_file(session, &file.path, &remote_target, job.overwrite_all, &ctx).await
        {
            emit_row(
                bus,
                &transfer_id,
                &file.name,
                TransferDirection::Upload,
                100,
                0,
                if e.to_string().starts_with("CONFLICT:") {
                    TransferStatus::Conflict
                } else {
                    TransferStatus::Error
                },
                &remote_target,
                &job.session_id,
                group_id,
                Some(e.to_string()),
            );
        }
        completed += 1;
        update_parent(bus, group_id, job, completed, TransferStatus::Transferring);
    }

    Ok(completed)
}

fn update_parent(
    bus: &crate::session::EventBus,
    group_id: Option<&str>,
    job: &TransferJob,
    completed: u64,
    status: TransferStatus,
) {
    let Some(gid) = group_id else { return };
    let label = match job.direction {
        TransferDirection::Upload => format!("上传 {} 项", job.items.len()),
        TransferDirection::Download => format!("下载 {} 项", job.items.len()),
    };
    emit_row(
        bus,
        gid,
        &label,
        job.direction,
        job.items.len() as u64,
        completed,
        status,
        &job.target_dir,
        &job.session_id,
        Some(gid),
        None,
    );
}

#[allow(clippy::too_many_arguments)]
async fn upload_folder_tar(
    session: &Arc<crate::session::LiveSession>,
    job: &TransferJob,
    folder: &FileInfo,
    remote_target: &str,
    transfer_id: &str,
    group_id: Option<&str>,
    bus: &crate::session::EventBus,
    cancelled: &crate::session::CancelSet,
) -> Result<()> {
    let stamp = format!(
        "{}-{:06}",
        crate::models::now_millis(),
        fastrand_u32() % 1_000_000
    );
    let local_sub_tmp = std::env::temp_dir().join(format!("tinyterm-pack-{stamp}"));
    let tmp_tar_local = local_sub_tmp.join(".tinyterm-pack.tar");
    let tmp_tar_remote = remote_fs::join_path(&job.target_dir, &format!(".tinyterm-pack-{stamp}.tar"));

    let result = async {
        tokio::fs::create_dir_all(&local_sub_tmp).await?;

        // stage 0 → 20: pack locally
        let source = folder.path.clone();
        let target = tmp_tar_local.clone();
        tokio::task::spawn_blocking(move || remote_fs::pack_local_dir(&source, &target.to_string_lossy()))
            .await??;
        emit_row(
            bus,
            transfer_id,
            &folder.name,
            TransferDirection::Upload,
            100,
            20,
            TransferStatus::Transferring,
            remote_target,
            &job.session_id,
            group_id,
            None,
        );

        // stage 20 → 80: upload the tarball (always overwrite)
        let ctx = ctx_for(
            job,
            transfer_id.to_string(),
            folder.name.clone(),
            20,
            60,
            remote_target.to_string(),
            group_id,
            bus,
            cancelled,
        );
        remote_fs::upload_file(
            session,
            &tmp_tar_local.to_string_lossy(),
            &tmp_tar_remote,
            true,
            &ctx,
        )
        .await?;

        // 90 % while the remote tar runs
        emit_row(
            bus,
            transfer_id,
            &folder.name,
            TransferDirection::Upload,
            100,
            90,
            TransferStatus::Transferring,
            remote_target,
            &job.session_id,
            group_id,
            None,
        );

        if job.overwrite_all {
            let _ = remote_fs::remove(session, remote_target, true).await;
        }
        let cmd = if job.overwrite_all {
            format!(
                "mkdir -p {dir} && tar -xf {tar} -C {dir}",
                dir = shell_quote(&job.target_dir),
                tar = shell_quote(&tmp_tar_remote)
            )
        } else {
            format!(
                "mkdir -p {dir} && tar -k -xf {tar} -C {dir}",
                dir = shell_quote(&job.target_dir),
                tar = shell_quote(&tmp_tar_remote)
            )
        };
        crate::ssh::exec_string(&session.handle, &cmd).await?;

        emit_row(
            bus,
            transfer_id,
            &folder.name,
            TransferDirection::Upload,
            100,
            100,
            TransferStatus::Done,
            remote_target,
            &job.session_id,
            group_id,
            None,
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;

    // best-effort cleanup
    let _ = crate::ssh::exec_string(
        &session.handle,
        &format!("rm -f {}", shell_quote(&tmp_tar_remote)),
    )
    .await;
    let _ = tokio::fs::remove_file(&tmp_tar_local).await;
    let _ = tokio::fs::remove_dir_all(&local_sub_tmp).await;

    result
}

#[allow(clippy::too_many_arguments)]
async fn upload_folder_files(
    session: &Arc<crate::session::LiveSession>,
    job: &TransferJob,
    folder: &FileInfo,
    remote_target: &str,
    transfer_id: &str,
    group_id: Option<&str>,
    bus: &crate::session::EventBus,
    cancelled: &crate::session::CancelSet,
) -> Result<()> {
    let _ = remote_fs::create_dir(session, remote_target).await;
    let mut tasks: Vec<(String, String)> = Vec::new();
    collect_upload_tasks(session, &folder.path, remote_target, &mut tasks).await?;

    if tasks.is_empty() {
        emit_row(
            bus,
            transfer_id,
            &folder.name,
            TransferDirection::Upload,
            1,
            1,
            TransferStatus::Done,
            remote_target,
            &job.session_id,
            group_id,
            None,
        );
        return Ok(());
    }

    let total = tasks.len() as u64;
    for (index, (local, remote)) in tasks.iter().enumerate() {
        let ctx = ctx_for(
            job,
            transfer_id.to_string(),
            folder.name.clone(),
            index as u64,
            1,
            remote_target.to_string(),
            group_id,
            bus,
            cancelled,
        );
        ctx.bus.send(AppEvent::Transfer(Box::new(TransferProgress {
            id: transfer_id.to_string(),
            file_name: folder.name.clone(),
            direction: TransferDirection::Upload,
            total,
            transferred: index as u64,
            status: TransferStatus::Transferring,
            error: None,
            target_path: Some(remote_target.to_string()),
            conflict_path: None,
            conflict_is_dir: false,
            session_id: Some(job.session_id.clone()),
            group_id: group_id.map(|s| s.to_string()),
            created_at_ms: crate::models::now_millis(),
        })));
        if let Err(e) = remote_fs::upload_file(session, local, remote, job.overwrite_all, &ctx).await
        {
            if e.to_string().starts_with("CONFLICT:") {
                emit_row(
                    bus,
                    transfer_id,
                    &folder.name,
                    TransferDirection::Upload,
                    total,
                    index as u64,
                    TransferStatus::Error,
                    remote_target,
                    &job.session_id,
                    group_id,
                    Some("存在同名文件，已跳过冲突项。可重试并选择全部覆盖。".into()),
                );
                return Ok(());
            }
            return Err(e);
        }
    }

    emit_row(
        bus,
        transfer_id,
        &folder.name,
        TransferDirection::Upload,
        total,
        total,
        TransferStatus::Done,
        remote_target,
        &job.session_id,
        group_id,
        None,
    );
    Ok(())
}

async fn collect_upload_tasks(
    session: &Arc<crate::session::LiveSession>,
    source_dir: &str,
    target_remote_dir: &str,
    out: &mut Vec<(String, String)>,
) -> Result<()> {
    let entries = crate::local_fs::list_dir(source_dir)?;
    for entry in entries {
        let remote_entry = remote_fs::join_path(target_remote_dir, &entry.name);
        if entry.is_dir {
            let _ = remote_fs::create_dir(session, &remote_entry).await;
            Box::pin(collect_upload_tasks(session, &entry.path, &remote_entry, out)).await?;
        } else {
            out.push((entry.path, remote_entry));
        }
    }
    Ok(())
}

async fn run_download(
    session: &Arc<crate::session::LiveSession>,
    job: &TransferJob,
    group_id: Option<&str>,
    can_tar: bool,
    bus: &crate::session::EventBus,
    cancelled: &crate::session::CancelSet,
) -> Result<u64> {
    let folders: Vec<&FileInfo> = job.items.iter().filter(|i| i.is_dir).collect();
    let files: Vec<&FileInfo> = job.items.iter().filter(|i| !i.is_dir).collect();
    let mut completed = 0u64;

    for folder in folders {
        let local_target = crate::local_fs::join_path(&job.target_dir, &folder.name);
        let transfer_id = format!("download:{local_target}");
        emit_row(
            bus,
            &transfer_id,
            &folder.name,
            TransferDirection::Download,
            100,
            0,
            TransferStatus::Pending,
            &local_target,
            &job.session_id,
            group_id,
            None,
        );

        if can_tar {
            if let Err(e) = download_folder_tar(
                session,
                job,
                folder,
                &local_target,
                &transfer_id,
                group_id,
                bus,
                cancelled,
            )
            .await
            {
                emit_row(
                    bus,
                    &transfer_id,
                    &folder.name,
                    TransferDirection::Download,
                    100,
                    0,
                    TransferStatus::Error,
                    &local_target,
                    &job.session_id,
                    group_id,
                    Some(e.to_string()),
                );
            }
        } else if let Err(e) = download_folder_files(
            session,
            job,
            folder,
            &local_target,
            &transfer_id,
            group_id,
            bus,
            cancelled,
        )
        .await
        {
            emit_row(
                bus,
                &transfer_id,
                &folder.name,
                TransferDirection::Download,
                100,
                0,
                TransferStatus::Error,
                &local_target,
                &job.session_id,
                group_id,
                Some(e.to_string()),
            );
        }
        completed += 1;
        update_parent(bus, group_id, job, completed, TransferStatus::Transferring);
    }

    for file in files {
        let local_target = crate::local_fs::join_path(&job.target_dir, &file.name);
        let transfer_id = format!("download:{local_target}");
        let ctx = ctx_for(
            job,
            transfer_id.clone(),
            file.name.clone(),
            0,
            100,
            local_target.clone(),
            group_id,
            bus,
            cancelled,
        );
        if let Err(e) =
            remote_fs::download_file(session, &file.path, &local_target, job.overwrite_all, &ctx).await
        {
            emit_row(
                bus,
                &transfer_id,
                &file.name,
                TransferDirection::Download,
                100,
                0,
                if e.to_string().starts_with("CONFLICT:") {
                    TransferStatus::Conflict
                } else {
                    TransferStatus::Error
                },
                &local_target,
                &job.session_id,
                group_id,
                Some(e.to_string()),
            );
        }
        completed += 1;
        update_parent(bus, group_id, job, completed, TransferStatus::Transferring);
    }

    Ok(completed)
}

#[allow(clippy::too_many_arguments)]
async fn download_folder_tar(
    session: &Arc<crate::session::LiveSession>,
    job: &TransferJob,
    folder: &FileInfo,
    local_target: &str,
    transfer_id: &str,
    group_id: Option<&str>,
    bus: &crate::session::EventBus,
    cancelled: &crate::session::CancelSet,
) -> Result<()> {
    let stamp = format!(
        "{}-{:06}",
        crate::models::now_millis(),
        fastrand_u32() % 1_000_000
    );
    let remote_parent = remote_fs::parent_of(&folder.path);
    let tmp_tar_remote = remote_fs::join_path(&remote_parent, &format!(".tinyterm-pack-{stamp}.tar"));
    let local_sub_tmp = std::env::temp_dir().join(format!("tinyterm-pack-{stamp}"));
    let tmp_tar_local = local_sub_tmp.join(".tinyterm-pack.tar");

    let result = async {
        tokio::fs::create_dir_all(&local_sub_tmp).await?;

        // stage 0 → 20: remote tar
        let cmd = format!(
            "tar -cf {tar} -C {parent} {name}",
            tar = shell_quote(&tmp_tar_remote),
            parent = shell_quote(&remote_parent),
            name = shell_quote(&folder.name)
        );
        crate::ssh::exec_string(&session.handle, &cmd).await?;
        emit_row(
            bus,
            transfer_id,
            &folder.name,
            TransferDirection::Download,
            100,
            20,
            TransferStatus::Transferring,
            local_target,
            &job.session_id,
            group_id,
            None,
        );

        // stage 20 → 80: download the tarball
        let ctx = ctx_for(
            job,
            transfer_id.to_string(),
            folder.name.clone(),
            20,
            60,
            local_target.to_string(),
            group_id,
            bus,
            cancelled,
        );
        remote_fs::download_file(
            session,
            &tmp_tar_remote,
            &tmp_tar_local.to_string_lossy(),
            true,
            &ctx,
        )
        .await?;

        // stage 80 → 100: unpack locally
        if job.overwrite_all {
            let target = local_target.to_string();
            let _ = tokio::task::spawn_blocking(move || crate::local_fs::delete(&target)).await;
        }
        let tar = tmp_tar_local.clone();
        let dest = job.target_dir.clone();
        let overwrite = job.overwrite_all;
        tokio::task::spawn_blocking(move || {
            remote_fs::unpack_local_dir(&tar.to_string_lossy(), &dest, overwrite)
        })
        .await??;

        emit_row(
            bus,
            transfer_id,
            &folder.name,
            TransferDirection::Download,
            100,
            100,
            TransferStatus::Done,
            local_target,
            &job.session_id,
            group_id,
            None,
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;

    let _ = crate::ssh::exec_string(
        &session.handle,
        &format!("rm -f {}", shell_quote(&tmp_tar_remote)),
    )
    .await;
    let _ = tokio::fs::remove_file(&tmp_tar_local).await;
    let _ = tokio::fs::remove_dir_all(&local_sub_tmp).await;

    result
}

#[allow(clippy::too_many_arguments)]
async fn download_folder_files(
    session: &Arc<crate::session::LiveSession>,
    job: &TransferJob,
    folder: &FileInfo,
    local_target: &str,
    transfer_id: &str,
    group_id: Option<&str>,
    bus: &crate::session::EventBus,
    cancelled: &crate::session::CancelSet,
) -> Result<()> {
    tokio::fs::create_dir_all(local_target).await.ok();
    let mut tasks: Vec<(String, String)> = Vec::new();
    collect_download_tasks(session, &folder.path, local_target, &mut tasks).await?;

    if tasks.is_empty() {
        emit_row(
            bus,
            transfer_id,
            &folder.name,
            TransferDirection::Download,
            1,
            1,
            TransferStatus::Done,
            local_target,
            &job.session_id,
            group_id,
            None,
        );
        return Ok(());
    }

    let total = tasks.len() as u64;
    for (index, (remote, local)) in tasks.iter().enumerate() {
        let ctx = ctx_for(
            job,
            transfer_id.to_string(),
            folder.name.clone(),
            index as u64,
            1,
            local_target.to_string(),
            group_id,
            bus,
            cancelled,
        );
        if let Err(e) = remote_fs::download_file(session, remote, local, job.overwrite_all, &ctx).await
        {
            if e.to_string().starts_with("CONFLICT:") {
                emit_row(
                    bus,
                    transfer_id,
                    &folder.name,
                    TransferDirection::Download,
                    total,
                    index as u64,
                    TransferStatus::Error,
                    local_target,
                    &job.session_id,
                    group_id,
                    Some("存在同名文件，已跳过冲突项。可重试并选择全部覆盖。".into()),
                );
                return Ok(());
            }
            return Err(e);
        }
    }

    emit_row(
        bus,
        transfer_id,
        &folder.name,
        TransferDirection::Download,
        total,
        total,
        TransferStatus::Done,
        local_target,
        &job.session_id,
        group_id,
        None,
    );
    Ok(())
}

async fn collect_download_tasks(
    session: &Arc<crate::session::LiveSession>,
    remote_dir: &str,
    local_dir: &str,
    out: &mut Vec<(String, String)>,
) -> Result<()> {
    let entries = remote_fs::list_dir(session, remote_dir).await?;
    for entry in entries {
        let local_entry = crate::local_fs::join_path(local_dir, &entry.name);
        if entry.is_dir {
            tokio::fs::create_dir_all(&local_entry).await.ok();
            Box::pin(collect_download_tasks(session, &entry.path, &local_entry, out)).await?;
        } else {
            out.push((entry.path, local_entry));
        }
    }
    Ok(())
}

fn fastrand_u32() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(crate::models::now_millis() as u64);
    hasher.finish() as u32
}




