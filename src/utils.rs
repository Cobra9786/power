use serde::Serialize;

#[derive(Serialize)]
pub struct AppInfo {
    pub app: &'static str,
    pub version: &'static str,
    pub build: &'static str,
}

#[macro_export]
macro_rules! get_app_info {
    () => {{
        const APP: &str = env!("CARGO_CRATE_NAME");
        const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

        #[inline]
        fn git_version() -> &'static str {
            option_env!("GIT_VERSION").unwrap_or("n/a")
        }

        $crate::utils::AppInfo {
            app: APP,
            version: PKG_VERSION,
            build: git_version(),
        }
    }};
}
