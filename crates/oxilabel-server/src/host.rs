//! The server's own machine (ADR 0003): whether a request comes from it, and the desktop's file
//! dialog for a folder or images, which only such requests may open.

use std::{convert::Infallible, net::SocketAddr, path::PathBuf};

use axum::{
    Json, Router,
    extract::{ConnectInfo, FromRequestParts, State},
    http::request::Parts,
    routing::{get, post},
};
use oxilabel_core::api::{FolderDialogReply, HostReply, ImagesDialogReply};

use crate::{AppState, HostPick, auth::Admin, error::ApiError};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/host", get(host))
        .route("/api/host/folder-dialog", post(folder_dialog))
        .route("/api/host/images-dialog", post(images_dialog))
}

/// Who sent a request: from the server's own machine (over loopback) or from elsewhere. A request
/// without a known peer address counts as from elsewhere.
#[derive(Debug, Clone, Copy)]
pub struct Caller {
    pub local: bool,
}

impl Caller {
    /// Fails unless the request comes from the server's machine, for things only its user may do.
    pub fn require_local(self, what: &str) -> Result<(), ApiError> {
        if self.local { Ok(()) } else { Err(ApiError::Forbidden(format!("{what} only works from the server's own machine"))) }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Infallible> {
        let peer = ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await.ok();
        Ok(Caller { local: peer.is_some_and(|ConnectInfo(peer)| peer.ip().is_loopback()) })
    }
}

async fn host(caller: Caller) -> Json<HostReply> {
    Json(HostReply { local: caller.local })
}

/// A picked path as the server stores it: canonical when it exists.
fn shown(path: PathBuf) -> String {
    path.canonicalize().unwrap_or(path).to_string_lossy().into_owned()
}

/// Opens the desktop's folder dialog on the server's machine and waits for the choice.
async fn folder_dialog(_: Admin, State(state): State<AppState>, caller: Caller) -> Result<Json<FolderDialogReply>, ApiError> {
    caller.require_local("the folder dialog")?;
    let dialog = state.host_dialog.clone().ok_or_else(|| ApiError::Conflict("this server cannot show a folder dialog".into()))?;
    let picked = dialog(HostPick::Folder).await.map_err(|reason| ApiError::Conflict(format!("the folder dialog could not open: {reason}")))?;
    Ok(Json(FolderDialogReply { path: picked.into_iter().next().map(shown) }))
}

/// Opens the desktop's file dialog for one or more images on the server's machine and waits for
/// the choice.
async fn images_dialog(_: Admin, State(state): State<AppState>, caller: Caller) -> Result<Json<ImagesDialogReply>, ApiError> {
    caller.require_local("the file dialog")?;
    let dialog = state.host_dialog.clone().ok_or_else(|| ApiError::Conflict("this server cannot show a file dialog".into()))?;
    let picked = dialog(HostPick::Images).await.map_err(|reason| ApiError::Conflict(format!("the file dialog could not open: {reason}")))?;
    Ok(Json(ImagesDialogReply { paths: picked.into_iter().map(shown).collect() }))
}
