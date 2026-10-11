//! Plugin management and contributed image commands share the host's inspectors.
use super::{plugins, workspace, workspace_form};
use std::path::PathBuf;
use windows::{
    Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    },
    core::{PWSTR, w},
};
pub(crate) fn manager() -> Result<(), String> {
    super::plugin_manager::open()
}
pub(crate) fn choose_package() -> Result<String, String> {
    let mut buffer = vec![0u16; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: workspace::root().unwrap_or_default(),
        lpstrFilter: w!("Panzo 插件包\0*.panzoplugin\0\0"),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: 32768,
        Flags: OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&raw mut dialog) }.as_bool() {
        return Ok("未选择插件包".into());
    }
    let len = buffer.iter().position(|v| *v == 0).unwrap_or(buffer.len());
    let path = PathBuf::from(String::from_utf16_lossy(&buffer[..len]));
    let package = plugins::read_package(&path)?;
    preview_install(package)
}
pub(crate) fn preview_install(package: panzo_plugin_api::Package) -> Result<String, String> {
    let manifest = &package.manifest;
    let description = format!(
        "{} {}\r\n作者声明：{}（未验证签名）\r\n{}\r\n这是原生程序，拥有当前用户权限；独立进程不等于安全沙箱。",
        manifest.name, manifest.version, manifest.publisher, manifest.description
    );
    let form = workspace_form::open(
        4,
        crate::platform::workspace_form::FormId::PluginInstall,
        "安装插件 · 确认来源",
        &description,
        &[
            ("插件 ID", manifest.id.clone()),
            ("文件 SHA-256", manifest.executable_sha256.clone()),
        ],
        &[],
        &[],
        &["信任此来源并安装", "取消"],
        Box::new(move |action, _| {
            if action == 2 {
                manager()?;
                return Ok("安装已取消".into());
            }
            plugins::with_store(|s| s.install(&package))?;
            super::image_workspace::refresh_plugins();
            workspace::notice(
                if package.manifest.protocol == panzo_plugin_api::media::PROTOCOL {
                    "媒体插件已安装，可在视频工作台使用声音、旁白和 AI 字幕"
                } else {
                    "插件已安装，可在图片工具中选择插件操作"
                },
            );
            super::plugin_manager::show_installed(&package.manifest.id)?;
            Ok("插件已安装".into())
        }),
    )?;
    for id in [100, 101] {
        unsafe {
            if let Ok(field) = windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(form), id) {
                windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                    field,
                    windows::Win32::UI::Controls::EM_SETREADONLY,
                    Some(windows::Win32::Foundation::WPARAM(1)),
                    None,
                );
            }
        }
    }
    Ok(String::new())
}
pub(crate) fn image_tools() -> Result<(), String> {
    let entries = plugins::external_commands();
    if entries.is_empty() {
        manager()?;
        return Ok(());
    }
    let commands = entries
        .iter()
        .map(|(_, _, name)| name.as_str())
        .collect::<Vec<_>>();
    let entries = entries.clone();
    workspace_form::open(
        0,
        crate::platform::workspace_form::FormId::ImagePlugin,
        "图片插件",
        "选择插件操作。使用点击运行时所选图片的编辑快照，输出添加为新图片；原图和撤销记录保留。",
        &[("操作", commands[0].into())],
        &[(0, &commands)],
        &[],
        &["运行插件", "管理插件"],
        Box::new(move |action, v| {
            if action == 2 {
                manager()?;
                return Ok(String::new());
            }
            let entry = entries
                .iter()
                .find(|(_, _, name)| name == &v[0])
                .ok_or("插件操作已变化，请重新选择")?;
            let (picture, document_id) = super::image_workspace::current_document()?;
            if super::image_workspace::is_busy(document_id) {
                return Err("此图片已有运行中的任务，请等待或取消".into());
            }
            let (image, revision) = super::capture_tools::workspace_snapshot(picture)?;
            let lease = plugins::acquire(&entry.0)?;
            let command = entry.1.clone();
            let token = workspace::start_job(
                workspace::Target::Image {
                    id: document_id,
                    revision,
                },
                &entry.2,
                move |cancel, _| {
                    super::plugin_runtime::execute(
                        &lease,
                        &command,
                        document_id,
                        revision,
                        &image,
                        &cancel,
                    )
                    .map(super::image_tasks::Output::Image)
                },
            );
            super::image_workspace::set_running(document_id, token);
            Ok("任务已提交，底部状态栏会提示结果。完成后可在图片列表选择新图片，素材库保留输出。切换标签不影响任务。".into())
        }),
    )?;
    Ok(())
}
