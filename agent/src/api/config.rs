/// Converts the agent's internal UI configuration into the public [`grey_api::UiConfig`] that the
/// SPA consumes. Kept separate from the request handlers so the entity modules stay focused.
impl From<&crate::config::UiConfig> for grey_api::UiConfig {
    fn from(config: &crate::config::UiConfig) -> Self {
        grey_api::UiConfig {
            title: config.title.clone(),
            logo: config.logo.clone(),
            links: config.links.clone(),
            reload_interval: config.reload_interval,
            // Expose only the public OIDC parameters the SPA needs for browser-side PKCE — never the
            // client secret (there is none) or the admin ACL.
            auth: config.admin.as_ref().map(|admin| grey_api::UiAuthConfig {
                issuer: admin.oidc.endpoint.clone(),
                client_id: admin.oidc.client_id.clone(),
                scopes: admin.oidc.scopes.clone(),
            }),
        }
    }
}
