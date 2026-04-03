use taurine_core::rpc::{
    ShutdownRequest, ShutdownResponse, StatusRequest, StatusResponse,
    daemon_control_server::DaemonControl,
};
use tokio::sync::mpsc;
use tonic::{Request, Response, Status};
use tracing::info;

pub struct DaemonService {
    shutdown_sender: mpsc::Sender<()>,
}

impl DaemonService {
    pub fn new(shutdown_sender: mpsc::Sender<()>) -> Self {
        Self { shutdown_sender }
    }
}

#[tonic::async_trait]
impl DaemonControl for DaemonService {
    async fn get_status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        Ok(Response::new(StatusResponse { online: true }))
    }

    async fn shutdown(
        &self,
        _request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        info!("Received gRPC shutdown request, signaling background process...");
        let _ = self.shutdown_sender.send(()).await;
        Ok(Response::new(ShutdownResponse { success: true }))
    }
}
