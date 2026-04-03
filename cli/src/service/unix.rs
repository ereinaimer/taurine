use service_manager::{
    ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx, ServiceStatus,
    ServiceStatusCtx, ServiceStopCtx, native_service_manager,
};
use std::env;
use tracing::{debug, error, info};

const TAURINE_SERVICE_LABEL: &str = "com.ereinaimer.taurine";

fn get_manager() -> Result<Box<dyn ServiceManager>, Box<dyn std::error::Error>> {
    let mut manager = native_service_manager().map_err(|e| {
        error!("Failed to initialize OS user service manager: {}", e);
        e
    })?;

    manager.set_level(ServiceLevel::User).map_err(|e| {
        error!("Failed to set service level: {}", e);
        e
    })?;
    Ok(manager)
}

pub fn up() -> Result<(), Box<dyn std::error::Error>> {
    let manager = get_manager()?;
    let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse()?;

    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::Running) => {
            info!("Taurine is already running.");
        }
        Ok(ServiceStatus::Stopped(_)) => {
            debug!("Taurine service found but stopped. Starting...");
            manager.start(ServiceStartCtx {
                label: label.clone(),
            })?;
            info!("Taurine started successfully.");
        }
        Ok(ServiceStatus::NotInstalled) | Err(_) => {
            debug!("Taurine service not found. Installing...");

            let current_exe = env::current_exe()?;

            manager.install(ServiceInstallCtx {
                label: label.clone(),
                program: current_exe,
                args: vec!["--daemon".into()],
                contents: None,
                username: None,
                working_directory: None,
                environment: None,
                autostart: true,
                restart_policy: Default::default(),
            })?;

            debug!("Install successful. Starting...");
            manager.start(ServiceStartCtx {
                label: label.clone(),
            })?;
            info!("Taurine started successfully.");
        }
    }

    Ok(())
}

pub fn down() -> Result<(), Box<dyn std::error::Error>> {
    // Placeholder INFO log until gRPC graceful shutdown is implemented
    debug!("TODO: Issue stop command to Taurine background process via gRPC.");

    // Fallback/Hard stop via service manager for now
    let manager = get_manager()?;
    let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse()?;

    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::Stopped(_)) | Ok(ServiceStatus::NotInstalled) | Err(_) => {
            info!("Taurine is already stopped.");
            return Ok(());
        }
        _ => {}
    }

    match manager.stop(ServiceStopCtx {
        label: label.clone(),
    }) {
        Ok(_) => info!("Taurine has been stopped."),
        Err(e) => error!("Failed to stop service: {}", e),
    }

    Ok(())
}

pub fn status() -> Result<(), Box<dyn std::error::Error>> {
    // Placeholder INFO log until gRPC status check is implemented
    debug!("TODO: Fetch detailed status (active snippets, uptime) via gRPC.");

    let manager = get_manager()?;
    let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse()?;

    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::Running) => info!("ONLINE (Running)"),
        Ok(ServiceStatus::Stopped(_)) => info!("OFFLINE (Stopped)"),
        Ok(ServiceStatus::NotInstalled) | Err(_) => info!("NOT INSTALLED"),
    }

    Ok(())
}
