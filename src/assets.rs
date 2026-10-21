use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            "git-terminal.png" => Some(Cow::Borrowed(include_bytes!("../assets/git-terminal.png"))),
            _ => None,
        })
    }
    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec!["git-terminal.png".into()])
    }
}
