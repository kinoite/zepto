use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use dirs;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub main_section: MainSection,
    #[serde(default)]
    pub editor_behavior: EditorBehavior,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            main_section: MainSection::default(),
            editor_behavior: EditorBehavior::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MainSection {
    #[serde(default = "default_background_color")]
    pub background_color: String,
    #[serde(default)]
    pub frame: Frame,
    #[serde(default)]
    pub line_numbers: LineNumbers,
    #[serde(default)]
    pub status_panel: StatusPanel,
    #[serde(default)]
    pub prompt_panel: PromptPanel,
}

impl Default for MainSection {
    fn default() -> Self {
        Self {
            background_color: default_background_color(),
            frame: Frame::default(),
            line_numbers: LineNumbers::default(),
            status_panel: StatusPanel::default(),
            prompt_panel: PromptPanel::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Frame {
    #[serde(default = "default_frame_corner")]
    pub corner: String,
    #[serde(default = "default_margin")]
    pub margin: u16,
    #[serde(default = "default_frame_color")]
    pub color: String,
    #[serde(default = "default_frame_hide")]
    pub hide: bool,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            corner: default_frame_corner(),
            margin: default_margin(),
            color: default_frame_color(),
            hide: default_frame_hide(),
        }
    }
}

fn default_frame_corner() -> String { "plain".to_string() }
fn default_margin() -> u16 { 0 }
fn default_frame_color() -> String { "#0000FF".to_string() }
fn default_frame_hide() -> bool { false }

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LineNumbers {
    #[serde(default = "default_line_numbers_enabled")]
    pub enabled: bool,
    #[serde(default = "default_line_numbers_color")]
    pub color: String,
    #[serde(default = "default_line_numbers_gutter_width")]
    pub gutter_width: u16,
    #[serde(default = "default_line_numbers_show_separator_line")]
    pub show_separator_line: bool,
}

impl Default for LineNumbers {
    fn default() -> Self {
        LineNumbers {
            enabled: default_line_numbers_enabled(),
            color: default_line_numbers_color(),
            gutter_width: default_line_numbers_gutter_width(),
            show_separator_line: default_line_numbers_show_separator_line(),
        }
    }
}

fn default_line_numbers_enabled() -> bool { true }
fn default_line_numbers_color() -> String { "#808080".to_string() }
fn default_line_numbers_gutter_width() -> u16 { 5 }
fn default_line_numbers_show_separator_line() -> bool { false }

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StatusPanel {
    #[serde(default = "default_status_panel_enabled")]
    pub enabled: bool,
    #[serde(default = "default_status_panel_background_color")]
    pub background_color: String,
    #[serde(default = "default_status_panel_foreground_color")]
    pub foreground_color: String,
}

impl Default for StatusPanel {
    fn default() -> Self {
        StatusPanel {
            enabled: default_status_panel_enabled(),
            background_color: default_status_panel_background_color(),
            foreground_color: default_status_panel_foreground_color(),
        }
    }
}

fn default_status_panel_enabled() -> bool { true }
fn default_status_panel_background_color() -> String { "#0000FF".to_string() }
fn default_status_panel_foreground_color() -> String { "#FFFFFF".to_string() }

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PromptPanel {
    #[serde(default = "default_prompt_panel_enabled")]
    pub enabled: bool,
    #[serde(default = "default_prompt_panel_background_color")]
    pub background_color: String,
    #[serde(default = "default_prompt_panel_foreground_color")]
    pub foreground_color: String,
}

impl Default for PromptPanel {
    fn default() -> Self {
        PromptPanel {
            enabled: default_prompt_panel_enabled(),
            background_color: default_prompt_panel_background_color(),
            foreground_color: default_prompt_panel_foreground_color(),
        }
    }
}

fn default_prompt_panel_enabled() -> bool { true }
fn default_prompt_panel_background_color() -> String { "#808080".to_string() }
fn default_prompt_panel_foreground_color() -> String { "#FFFFFF".to_string() }

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EditorBehavior {
    #[serde(default = "default_vim_mode")]
    pub vim: bool,
}

impl Default for EditorBehavior {
    fn default() -> Self {
        Self {
            vim: false,
        }
    }
}

fn default_vim_mode() -> bool {
    false
}

fn default_background_color() -> String { "#000000".to_string() }

pub fn load_config() -> Config {
    let config_path = if let Some(config_dir) = dirs::config_dir() {
        let mut path = config_dir;
        path.push("zepto");
        path.push("config.toml");
        path
    } else {
        PathBuf::from("config.toml")
    };

    println!("Attempting to load config from: {}", config_path.display());

    match fs::read_to_string(&config_path) {
        Ok(content) => {
            match toml::from_str(&content) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("Error parsing config.toml: {}. Using default configuration.", e);
                    Config::default()
                }
            }
        },
        Err(e) => {
            eprintln!("Could not read config file at {}: {}. Using default configuration.", config_path.display(), e);
            if let Some(mut default_config_dir) = dirs::config_dir() {
                default_config_dir.push("zepto");
                if let Err(create_err) = fs::create_dir_all(&default_config_dir) {
                    eprintln!("Error creating config directory {:?}: {}", default_config_dir, create_err);
                } else {
                    let default_config = toml::to_string_pretty(&Config::default()).unwrap();
                    let full_config_path = default_config_dir.join("config.toml");
                    if let Err(write_err) = fs::write(&full_config_path, default_config) {
                        eprintln!("Error writing default config to {}: {}", full_config_path.display(), write_err);
                    } else {
                        println!("Created default config file at {}", full_config_path.display());
                    }
                }
            }
            Config::default()
        }
    }
}
