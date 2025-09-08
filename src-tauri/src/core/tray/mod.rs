use once_cell::sync::OnceCell;
use tauri::tray::TrayIconBuilder;
use tauri::Emitter;
#[cfg(target_os = "macos")]
pub mod speed_rate;
use crate::ipc::Rate;
use crate::process::AsyncHandler;
use crate::{
    cmd,
    config::Config,
    feat,
    ipc::IpcManager,
    logging,
    module::lightweight::is_in_lightweight_mode,
    singleton_lazy,
    utils::{dirs::find_target_icons, i18n::t},
    Type,
};

use super::handle;
use anyhow::Result;
use futures::future::join_all;
use parking_lot::Mutex;
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, Menu},
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
    AppHandle, Wry,
};

#[derive(Clone)]
struct TrayState {}

// 托盘点击防抖机制
static TRAY_CLICK_DEBOUNCE: OnceCell<Mutex<Instant>> = OnceCell::new();
const TRAY_CLICK_DEBOUNCE_MS: u64 = 300;

fn get_tray_click_debounce() -> &'static Mutex<Instant> {
    TRAY_CLICK_DEBOUNCE.get_or_init(|| Mutex::new(Instant::now() - Duration::from_secs(1)))
}

fn should_handle_tray_click() -> bool {
    let debounce_lock = get_tray_click_debounce();
    let mut last_click = debounce_lock.lock();
    let now = Instant::now();

    if now.duration_since(*last_click) >= Duration::from_millis(TRAY_CLICK_DEBOUNCE_MS) {
        *last_click = now;
        true
    } else {
        log::debug!(target: "app", "托盘点击被防抖机制忽略，距离上次点击 {:?}ms", 
                  now.duration_since(*last_click).as_millis());
        false
    }
}

#[cfg(target_os = "macos")]
pub struct Tray {
    last_menu_update: Mutex<Option<Instant>>,
    menu_updating: AtomicBool,
}

#[cfg(not(target_os = "macos"))]
pub struct Tray {
    last_menu_update: Mutex<Option<Instant>>,
    menu_updating: AtomicBool,
}

impl TrayState {
    pub async fn get_common_tray_icon() -> (bool, Vec<u8>) {
        let verge = Config::verge().await.latest_ref().clone();
        let is_common_tray_icon = verge.common_tray_icon.unwrap_or(false);
        if is_common_tray_icon {
            if let Ok(Some(common_icon_path)) = find_target_icons("common") {
                if let Ok(icon_data) = fs::read(common_icon_path) {
                    return (true, icon_data);
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            let tray_icon_colorful = verge.tray_icon.unwrap_or("monochrome".to_string());
            if tray_icon_colorful == "monochrome" {
                (
                    false,
                    include_bytes!("../../../icons/tray-icon-mono.ico").to_vec(),
                )
            } else {
                (
                    false,
                    include_bytes!("../../../icons/tray-icon.ico").to_vec(),
                )
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            (
                false,
                include_bytes!("../../../icons/tray-icon.ico").to_vec(),
            )
        }
    }

    pub async fn get_sysproxy_tray_icon() -> (bool, Vec<u8>) {
        let verge = Config::verge().await.latest_ref().clone();
        let is_sysproxy_tray_icon = verge.sysproxy_tray_icon.unwrap_or(false);
        if is_sysproxy_tray_icon {
            if let Ok(Some(sysproxy_icon_path)) = find_target_icons("sysproxy") {
                if let Ok(icon_data) = fs::read(sysproxy_icon_path) {
                    return (true, icon_data);
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            let tray_icon_colorful = verge.tray_icon.clone().unwrap_or("monochrome".to_string());
            if tray_icon_colorful == "monochrome" {
                (
                    false,
                    include_bytes!("../../../icons/tray-icon-sys-mono-new.ico").to_vec(),
                )
            } else {
                (
                    false,
                    include_bytes!("../../../icons/tray-icon-sys.ico").to_vec(),
                )
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            (
                false,
                include_bytes!("../../../icons/tray-icon-sys.ico").to_vec(),
            )
        }
    }

    pub async fn get_tun_tray_icon() -> (bool, Vec<u8>) {
        let verge = Config::verge().await.latest_ref().clone();
        let is_tun_tray_icon = verge.tun_tray_icon.unwrap_or(false);
        if is_tun_tray_icon {
            if let Ok(Some(tun_icon_path)) = find_target_icons("tun") {
                if let Ok(icon_data) = fs::read(tun_icon_path) {
                    return (true, icon_data);
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            let tray_icon_colorful = verge.tray_icon.clone().unwrap_or("monochrome".to_string());
            if tray_icon_colorful == "monochrome" {
                (
                    false,
                    include_bytes!("../../../icons/tray-icon-tun-mono-new.ico").to_vec(),
                )
            } else {
                (
                    false,
                    include_bytes!("../../../icons/tray-icon-tun.ico").to_vec(),
                )
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            (
                false,
                include_bytes!("../../../icons/tray-icon-tun.ico").to_vec(),
            )
        }
    }
}

impl Default for Tray {
    fn default() -> Self {
        Tray {
            last_menu_update: Mutex::new(None),
            menu_updating: AtomicBool::new(false),
        }
    }
}

// Use simplified singleton_lazy macro
singleton_lazy!(Tray, TRAY, Tray::default);

impl Tray {
    pub async fn init(&self) -> Result<()> {
        let app_handle = handle::Handle::global()
            .app_handle()
            .ok_or_else(|| anyhow::anyhow!("Failed to get app handle for tray initialization"))?;
        self.create_tray_from_handle(&app_handle).await?;
        Ok(())
    }

    /// 更新托盘点击行为
    pub async fn update_click_behavior(&self) -> Result<()> {
        Ok(())
    }

    /// 更新托盘菜单
    pub async fn update_menu(&self) -> Result<()> {
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    pub async fn update_icon(&self, _rate: Option<Rate>) -> Result<()> {
        let app_handle = match handle::Handle::global().app_handle() {
            Some(handle) => handle,
            None => {
                log::warn!(target: "app", "更新托盘图标失败: app_handle不存在");
                return Ok(());
            }
        };

        let tray = match app_handle.tray_by_id("main") {
            Some(tray) => tray,
            None => {
                log::warn!(target: "app", "更新托盘图标失败: 托盘不存在");
                return Ok(());
            }
        };

        let verge = Config::verge().await.latest_ref().clone();
        let system_mode = verge.enable_system_proxy.as_ref().unwrap_or(&false);
        let tun_mode = verge.enable_tun_mode.as_ref().unwrap_or(&false);

        let (_is_custom_icon, icon_bytes) = match (*system_mode, *tun_mode) {
            (true, true) => TrayState::get_tun_tray_icon().await,
            (true, false) => TrayState::get_sysproxy_tray_icon().await,
            (false, true) => TrayState::get_tun_tray_icon().await,
            (false, false) => TrayState::get_common_tray_icon().await,
        };

        let _ = tray.set_icon(Some(tauri::image::Image::from_bytes(&icon_bytes)?));
        Ok(())
    }

    /// 更新托盘显示状态的函数
    pub async fn update_tray_display(&self) -> Result<()> {
        let app_handle = handle::Handle::global()
            .app_handle()
            .ok_or_else(|| anyhow::anyhow!("Failed to get app handle for tray update"))?;
        let _tray = app_handle
            .tray_by_id("main")
            .ok_or_else(|| anyhow::anyhow!("Failed to get main tray"))?;

        // 更新菜单
        self.update_menu().await?;

        Ok(())
    }

    /// 更新托盘提示
    pub async fn update_tooltip(&self) -> Result<()> {
        let app_handle = match handle::Handle::global().app_handle() {
            Some(handle) => handle,
            None => {
                log::warn!(target: "app", "更新托盘提示失败: app_handle不存在");
                return Ok(());
            }
        };

        let verge = Config::verge().await.latest_ref().clone();
        let system_proxy = verge.enable_system_proxy.as_ref().unwrap_or(&false);
        let tun_mode = verge.enable_tun_mode.as_ref().unwrap_or(&false);

        let switch_map = {
            let mut map = std::collections::HashMap::new();
            map.insert(true, "on");
            map.insert(false, "off");
            map
        };

        let mut current_profile_name = "None".to_string();
        {
            let profiles = Config::profiles().await;
            let profiles = profiles.latest_ref();
            if let Some(current_profile_uid) = profiles.get_current() {
                if let Ok(profile) = profiles.get_item(&current_profile_uid) {
                    current_profile_name = match &profile.name {
                        Some(profile_name) => profile_name.to_string(),
                        None => current_profile_name,
                    };
                }
            }
        }

        // Get localized strings before using them
        let sys_proxy_text = t("SysProxy").await;
        let tun_text = t("TUN").await;
        let profile_text = t("Profile").await;

        let version = env!("CARGO_PKG_VERSION");
        if let Some(tray) = app_handle.tray_by_id("main") {
            let _ = tray.set_tooltip(Some(&format!(
                "Clash Verge {version}\n{}: {}\n{}: {}\n{}: {}",
                sys_proxy_text,
                switch_map[system_proxy],
                tun_text,
                switch_map[tun_mode],
                profile_text,
                current_profile_name
            )));
        } else {
            log::warn!(target: "app", "更新托盘提示失败: 托盘不存在");
        }

        Ok(())
    }

    pub async fn update_part(&self) -> Result<()> {
        // self.update_menu().await?;
        // 更新轻量模式显示状态
        self.update_tray_display().await?;
        self.update_icon(None).await?;
        self.update_tooltip().await?;
        Ok(())
    }

    pub async fn create_tray_from_handle(&self, app_handle: &AppHandle) -> Result<()> {
        log::info!(target: "app", "正在从AppHandle创建系统托盘");

        // 获取图标
        let icon_bytes = TrayState::get_common_tray_icon().await.1;
        let icon = tauri::image::Image::from_bytes(&icon_bytes)?;

        #[cfg(not(target_os = "linux"))]
        let mut builder = TrayIconBuilder::with_id("main")
            .icon(icon)
            .icon_as_template(false);


        let quit_i = MenuItem::with_id(app_handle, "quit", "Quit", true, None::<&str>)?;
        let menu = Menu::with_items(app_handle, &[&quit_i])?;
        let tray = builder.show_menu_on_left_click(false).build(app_handle)?;
        tray.set_menu(Some(menu));

        
        tray.on_tray_icon_event(|_app_handle, event| {
            AsyncHandler::spawn(|| async move {
                let tray_event = { Config::verge().await.latest_ref().tray_event.clone() };
                let tray_event: String = tray_event.unwrap_or("main_window".into());
                log::debug!(target: "app", "tray event: {tray_event:?}");

                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Down,
                    ..
                } = event
                {
                    // 添加防抖检查，防止快速连击
                    if !should_handle_tray_click() {
                        return;
                    }

                    use std::future::Future;
                    use std::pin::Pin;

                    let fut: Pin<Box<dyn Future<Output = ()> + Send>> = match tray_event.as_str() {
                        "system_proxy" => Box::pin(async move {
                            feat::toggle_system_proxy().await;
                        }),
                        "tun_mode" => Box::pin(async move {
                            feat::toggle_tun_mode(None).await;
                        }),
                        "main_window" => Box::pin(async move {
                            use crate::utils::window_manager::WindowManager;
                            log::info!(target: "app", "Tray点击事件: 显示主窗口");
                            if crate::module::lightweight::is_in_lightweight_mode() {
                                log::info!(target: "app", "当前在轻量模式，正在退出轻量模式");
                                crate::module::lightweight::exit_lightweight_mode().await;
                            }
                            let result = WindowManager::show_main_window().await;
                            log::info!(target: "app", "窗口显示结果: {result:?}");
                        }),
                        _ => Box::pin(async move {}),
                    };
                    fut.await;
                }
            });
        });
        tray.on_menu_event(on_menu_event);
        log::info!(target: "app", "系统托盘创建成功");
        Ok(())
    }

    // 托盘统一的状态更新函数
    pub async fn update_all_states(&self) -> Result<()> {
        // 确保所有状态更新完成
        self.update_tray_display().await?;
        // self.update_menu().await?;
        self.update_icon(None).await?;
        self.update_tooltip().await?;

        Ok(())
    }
}

fn on_menu_event(_: &AppHandle, event: MenuEvent) {
    AsyncHandler::spawn(|| async move {
        match event.id.as_ref() {
            "quit" => {
                feat::quit().await; // Await async function
            }
            _ => {
                // Ignore other menu events
            }
        }
    });
}
