mod accessibility;
pub mod apartment;
mod app_icon;
mod app_menu;
mod app_shell;
pub mod background_image;
pub mod camera_regeneration;
pub mod capture_probe;
pub(crate) mod capture_tools;
pub mod compositor;
pub mod d3d11;
mod decode_worker;
mod edit_gestures;
pub mod editor;
mod editor_clock;
mod editor_dialog;
mod editor_load;
mod editor_tasks;
pub mod editor_timeline;
pub mod editor_transaction;
pub mod editor_ui;
pub mod export;
mod export_window;
pub mod fmp4_writer;
mod inline_input;
mod hover_tip;
pub mod input_capture;
pub mod manifest_store;
pub mod media_decoder;
pub mod media_foundation;
pub mod preflight;
mod presentation_metrics;
pub mod preview;
pub mod preview_quality;
pub mod preview_proxy;
mod preview_scheduler;
pub mod preview_window;
pub mod qpc;
pub mod recorder;
pub mod recorder_window;
pub mod recording_editing_probe;
pub mod recovery;
pub mod scrub_preview;
mod shortcut_settings;
mod shortcuts;
pub mod stability;
pub mod stability_stimulus;
mod timeline_thumbnails;
mod timer_resolution;
pub mod ui_preferences;
pub mod video_processor;
pub mod window_capture;

pub mod audio;
pub mod audio_capture;
pub mod audio_decode;
mod audio_preview;
pub mod media_import;

mod media_settings;
mod recording_hud;

pub mod ocr;

mod image_ops;

mod image_tasks;
mod image_workflows;

mod gif_player;

mod video_annotations;

mod vision;
mod vision_settings;
mod vision_ui;

mod image_icons;
mod image_workspace;
mod workspace;
mod workspace_form;
mod workspace_image_tools;
mod workspace_settings;
mod workspace_ui;

mod plugin_runtime;
mod plugin_ui;
mod plugins;

mod desktop_ui;

mod plugin_manager;

mod asset_index;
mod asset_preview;
mod management_window;
mod path_field;

mod collage_model;
mod collage_workspace;

mod media_plugin_runtime;
pub mod media_worker;
mod speech_settings;

mod media_jobs;
mod media_panel;
mod workbench_overlays;

#[cfg(test)]
mod media_acceptance;

mod app_dialog;

mod system_preferences;
mod ui_motion;

mod workbench_layout;
mod pane_splitter;

mod asset_audio;

mod font_catalog;

mod recording_setup;

mod project_assets;

mod timeline_visual;

mod visual_panel;
