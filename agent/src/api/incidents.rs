use actix_web::{HttpResponse, Result, web};
use grey_api::Identifier;
use serde::Deserialize;

use super::AppState;
use crate::state::{DEFAULT_INCIDENT_PAGE, IncidentStore};

/// Query parameters for the paginated public incident list.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Page size (clamped to a sane maximum). Defaults to [`DEFAULT_INCIDENT_PAGE`].
    pub limit: Option<usize>,
    /// The id returned as `next_cursor` by the previous page; continues with older incidents.
    pub cursor: Option<String>,
}

/// `GET /api/v1/incidents?limit=&cursor=` — a page of publicly visible incidents, newest-first, each
/// with its updates embedded (so the UI needs no follow-up calls). Hidden/draft incidents are
/// excluded from this unauthenticated view.
pub async fn get_incidents(
    data: web::Data<AppState>,
    query: web::Query<ListQuery>,
) -> Result<HttpResponse> {
    let limit = query.limit.unwrap_or(DEFAULT_INCIDENT_PAGE).clamp(1, 100);
    let cursor = query.cursor.as_deref().and_then(Identifier::parse);
    let page = data.state.list_incidents(false, limit, cursor).await?;
    Ok(HttpResponse::Ok().json(page))
}

#[cfg(test)]
mod tests {
    use actix_web::body::MessageBody;
    use actix_web::http::StatusCode;
    use grey_api::{Impact, IncidentsPage};
    use tempfile::tempdir;

    use super::*;

    fn query(limit: Option<usize>, cursor: Option<String>) -> web::Query<ListQuery> {
        web::Query(ListQuery { limit, cursor })
    }

    #[actix_web::test]
    async fn empty_when_no_incidents() {
        let temp_dir = tempdir().unwrap();
        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;
        let resp = get_incidents(web::Data::new(app_state), query(None, None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().try_into_bytes().unwrap();
        let page: IncidentsPage = serde_json::from_slice(&body).unwrap();
        assert!(page.incidents.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[actix_web::test]
    async fn exposes_only_visible_with_embedded_updates() {
        let temp_dir = tempdir().unwrap();
        let app_state = AppState::test(temp_dir.path().to_path_buf()).await;

        let visible = app_state
            .state
            .create_incident("Visible".into(), Impact::Offline, "down".into())
            .await
            .unwrap();
        app_state
            .state
            .create_incident("Hidden".into(), Impact::Hidden, "draft".into())
            .await
            .unwrap();

        let resp = get_incidents(web::Data::new(app_state), query(None, None)).await.unwrap();
        let body = resp.into_body().try_into_bytes().unwrap();
        let page: IncidentsPage = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            page.incidents.iter().map(|v| v.id()).collect::<Vec<_>>(),
            vec![visible.id()],
            "the public endpoint hides draft incidents"
        );
        assert_eq!(page.incidents[0].updates.len(), 1, "updates are embedded in the response");
    }
}
