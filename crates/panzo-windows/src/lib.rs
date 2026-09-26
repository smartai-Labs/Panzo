//! Windows-specific capture, input, graphics, and media integration.

#[cfg(target_os = "windows")]
pub mod platform;

#[cfg(not(target_os = "windows"))]
compile_error!("panzo-windows only supports Windows");
