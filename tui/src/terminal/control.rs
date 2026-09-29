use crate::terminal::status::{DaemonStatus, probe_daemon_status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleAction {
    Start,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LifecycleOutcome {
    pub(crate) action: LifecycleAction,
    pub(crate) status: DaemonStatus,
}

pub(crate) trait DaemonController {
    fn start(&self) -> taurine_core::Result<()>;
    fn stop(&self) -> taurine_core::Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct SystemDaemonController;

impl DaemonController for SystemDaemonController {
    fn start(&self) -> taurine_core::Result<()> {
        let start_on_boot = {
            use taurine_core::db::init;
            use taurine_core::settings::SettingsManager;

            let conn = init::setup()?;
            SettingsManager::new(&conn).load_all().start_on_boot
        };

        taurine_core::service::up(start_on_boot)
    }

    fn stop(&self) -> taurine_core::Result<()> {
        taurine_core::service::down()
    }
}

pub(crate) const fn action_for_status(status: DaemonStatus) -> LifecycleAction {
    match status {
        DaemonStatus::Stopped | DaemonStatus::Stopping => LifecycleAction::Start,
        DaemonStatus::Running | DaemonStatus::Paused | DaemonStatus::Starting => {
            LifecycleAction::Stop
        }
    }
}

pub(crate) const fn transition_status_for_action(action: LifecycleAction) -> DaemonStatus {
    match action {
        LifecycleAction::Start => DaemonStatus::Starting,
        LifecycleAction::Stop => DaemonStatus::Stopping,
    }
}

pub(crate) fn toggle_daemon<C: DaemonController>(
    controller: &C,
    current_status: DaemonStatus,
) -> taurine_core::Result<LifecycleOutcome> {
    let action = action_for_status(current_status);

    let status = match action {
        LifecycleAction::Start => {
            controller.start()?;
            Some(DaemonStatus::Starting)
        }
        LifecycleAction::Stop => {
            controller.stop()?;
            None
        }
    };

    Ok(LifecycleOutcome {
        action,
        status: status.unwrap_or_else(probe_daemon_status),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Once;

    use super::*;

    static MOCK_KEYRING: Once = Once::new();

    fn install_mock_keyring() {
        MOCK_KEYRING.call_once(|| {
            keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        });
    }

    #[derive(Default)]
    struct MockController {
        start_calls: Cell<usize>,
        stop_calls: Cell<usize>,
    }

    impl DaemonController for MockController {
        fn start(&self) -> taurine_core::Result<()> {
            self.start_calls.set(self.start_calls.get() + 1);
            Ok(())
        }

        fn stop(&self) -> taurine_core::Result<()> {
            self.stop_calls.set(self.stop_calls.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn running_maps_to_stop() {
        assert_eq!(
            action_for_status(DaemonStatus::Running),
            LifecycleAction::Stop
        );
    }

    #[test]
    fn paused_maps_to_stop() {
        assert_eq!(
            action_for_status(DaemonStatus::Paused),
            LifecycleAction::Stop
        );
    }

    #[test]
    fn stopped_maps_to_start() {
        assert_eq!(
            action_for_status(DaemonStatus::Stopped),
            LifecycleAction::Start
        );
    }

    #[test]
    fn stopped_toggle_uses_start_path() {
        let controller = MockController::default();
        let _ = toggle_daemon(&controller, DaemonStatus::Stopped);
        assert_eq!(controller.start_calls.get(), 1);
        assert_eq!(controller.stop_calls.get(), 0);
    }

    #[test]
    fn running_toggle_uses_stop_path() {
        install_mock_keyring();
        let controller = MockController::default();
        let _ = toggle_daemon(&controller, DaemonStatus::Running);
        assert_eq!(controller.start_calls.get(), 0);
        assert_eq!(controller.stop_calls.get(), 1);
    }

    #[test]
    fn paused_toggle_uses_stop_path() {
        install_mock_keyring();
        let controller = MockController::default();
        let _ = toggle_daemon(&controller, DaemonStatus::Paused);
        assert_eq!(controller.start_calls.get(), 0);
        assert_eq!(controller.stop_calls.get(), 1);
    }
}
