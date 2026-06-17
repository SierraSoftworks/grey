use grey_api::AdminUser;
use yew::prelude::*;

use crate::api::ApiClient;
use crate::contexts::use_ui_config;

/// Authentication state shared with the component tree. `user` is `Some` when an administrator is
/// signed in; `configured` reflects whether OIDC is set up at all (so the UI knows to offer login).
#[derive(Clone, PartialEq)]
pub struct AuthContext {
    pub user: Option<AdminUser>,
    pub configured: bool,
    pub token: Option<String>,
    /// A ready-to-use API client (with session renewal wired in) for descendants to make calls.
    pub client: ApiClient,
    pub login: Callback<()>,
    pub logout: Callback<()>,
}

impl AuthContext {
    pub fn is_authenticated(&self) -> bool {
        self.user.is_some()
    }
}

#[derive(Properties, PartialEq)]
pub struct AuthProviderProps {
    pub children: Children,
}

#[function_component(AuthProvider)]
pub fn auth_provider(props: &AuthProviderProps) -> Html {
    let config_ctx = use_ui_config();
    let auth_cfg = config_ctx.config.auth.clone();
    let client = ApiClient::new(auth_cfg.clone());
    let user = use_state(|| Option::<AdminUser>::None);
    let token = use_state(|| Option::<String>::None);

    {
        let user = user.clone();
        let token = token.clone();
        let auth_cfg = auth_cfg.clone();
        let client = client.clone();
        // On mount, finish any pending OIDC callback, then validate the stored token by fetching the
        // current user. Effects only run client-side, so this never executes during SSR.
        use_effect_with((), move |_| {
            #[cfg(feature = "wasm")]
            if let Some(cfg) = auth_cfg {
                wasm_bindgen_futures::spawn_local(async move {
                    let mut current = crate::auth::stored_token();
                    if crate::auth::has_pending_callback() {
                        match crate::auth::complete_callback(&cfg).await {
                            Ok(Some(t)) => current = Some(t),
                            Ok(None) => {}
                            Err(err) => {
                                gloo::console::error!(format!("OIDC sign-in failed: {err}"))
                            }
                        }
                    }
                    if current.is_some() {
                        // `me` transparently renews an expired token before failing, so a successful
                        // result means the session is valid.
                        match client.me().await {
                            Ok(u) => {
                                token.set(crate::auth::stored_token());
                                user.set(Some(u));
                            }
                            // The stored token is no longer accepted; drop it.
                            Err(_) => crate::auth::clear_token(),
                        }
                    }
                });
            }
            #[cfg(not(feature = "wasm"))]
            let _ = (&user, &token, &auth_cfg, &client);
            || ()
        });
    }

    let login = {
        let auth_cfg = auth_cfg.clone();
        let user = user.clone();
        let token = token.clone();
        let client = client.clone();
        Callback::from(move |_| {
            #[cfg(feature = "wasm")]
            if let Some(cfg) = auth_cfg.clone() {
                let user = user.clone();
                let token = token.clone();
                let client = client.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match crate::auth::begin_login(&cfg).await {
                        Ok(Some(_)) => match client.me().await {
                            Ok(u) => {
                                token.set(crate::auth::stored_token());
                                user.set(Some(u));
                            }
                            Err(err) => {
                                gloo::console::error!(format!("Sign-in validation failed: {err}"));
                                crate::auth::clear_token();
                            }
                        },
                        Ok(None) => {}
                        Err(err) => gloo::console::error!(format!("Sign-in failed: {err}")),
                    }
                });
            }
            #[cfg(not(feature = "wasm"))]
            let _ = (&auth_cfg, &user, &token, &client);
        })
    };

    let logout = {
        let user = user.clone();
        let token = token.clone();
        Callback::from(move |_| {
            crate::auth::clear_token();
            user.set(None);
            token.set(None);
        })
    };

    let context = AuthContext {
        user: (*user).clone(),
        configured: auth_cfg.is_some(),
        token: (*token).clone(),
        client,
        login,
        logout,
    };

    html! {
        <ContextProvider<AuthContext> context={context}>
            { props.children.clone() }
        </ContextProvider<AuthContext>>
    }
}

#[hook]
pub fn use_auth() -> AuthContext {
    use_context::<AuthContext>()
        .expect("AuthContext not found. Make sure to wrap your component with AuthProvider.")
}
