use ksni;
use ksni::TrayMethods;
use std::process::Command;
use std::env;

#[derive(Debug)]
pub struct MimickTray {}

impl ksni::Tray for MimickTray {
    fn id(&self) -> String {
        "mimick_tray".to_string()
    }

    fn icon_name(&self) -> String {
        "folder-sync".to_string()
    }

    fn title(&self) -> String {
        "Mimick Sync".into()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Settings".into(),
                activate: Box::new(|_| {
                    if let Ok(exe) = env::current_exe() {
                        let _ = Command::new(exe).arg("--settings").spawn();
                    }
                }),
                ..Default::default()
            }.into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|_| {
                    std::process::exit(0);
                }),
                ..Default::default()
            }.into(),
        ]
    }
}

pub async fn build_tray() -> Result<ksni::Handle<MimickTray>, ksni::Error> {
    MimickTray {}.spawn().await
}
