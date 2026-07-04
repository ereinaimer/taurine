use std::time::Duration;

use ratatui::style::{Color, Style};
use taurine_core::rpc::{StatusRequest, daemon_control_client::DaemonControlClient, get_rpc_url};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DaemonStatus {
    Running,
    #[default]
    Stopped,
    Paused,
    Starting,
    Stopping,
}

impl DaemonStatus {
    pub(crate) const fn from_flags(online: bool, paused: bool) -> Self {
        if !online {
            Self::Stopped
        } else if paused {
            Self::Paused
        } else {
            Self::Running
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Paused => "paused",
            Self::Starting => "starting",
            Self::Stopping => "stopping",
        }
    }

    pub(crate) const fn color(self) -> Color {
        match self {
            Self::Running => Color::Green,
            Self::Stopped => Color::Red,
            Self::Paused => Color::Yellow,
            Self::Starting => Color::Cyan,
            Self::Stopping => Color::LightYellow,
        }
    }

    pub(crate) fn style(self) -> Style {
        Style::default().fg(self.color())
    }

    pub(crate) const fn is_transitioning(self) -> bool {
        matches!(self, Self::Starting | Self::Stopping)
    }
}

pub(crate) fn probe_daemon_status() -> DaemonStatus {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return DaemonStatus::Stopped;
    };

    runtime.block_on(async {
        tokio::time::timeout(Duration::from_millis(250), async {
            let Ok(mut client) = DaemonControlClient::connect(get_rpc_url()).await else {
                return None;
            };
            let Ok(response) = client
                .get_status(tonic::Request::new(StatusRequest {}))
                .await
            else {
                return None;
            };
            let status = response.into_inner();
            Some(DaemonStatus::from_flags(status.online, status.paused))
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(DaemonStatus::Stopped)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_label_is_lowercase_running() {
        assert_eq!(DaemonStatus::Running.label(), "running");
    }

    #[test]
    fn stopped_label_is_lowercase_stopped() {
        assert_eq!(DaemonStatus::Stopped.label(), "stopped");
    }

    #[test]
    fn paused_label_is_lowercase_paused() {
        assert_eq!(DaemonStatus::Paused.label(), "paused");
    }

    #[test]
    fn starting_label_is_lowercase_starting() {
        assert_eq!(DaemonStatus::Starting.label(), "starting");
    }

    #[test]
    fn stopping_label_is_lowercase_stopping() {
        assert_eq!(DaemonStatus::Stopping.label(), "stopping");
    }

    #[test]
    fn status_color_mapping_matches_design() {
        assert_eq!(DaemonStatus::Running.color(), Color::Green);
        assert_eq!(DaemonStatus::Stopped.color(), Color::Red);
        assert_eq!(DaemonStatus::Paused.color(), Color::Yellow);
        assert_eq!(DaemonStatus::Starting.color(), Color::Cyan);
        assert_eq!(DaemonStatus::Stopping.color(), Color::LightYellow);
    }

    #[test]
    fn status_flags_map_to_running_paused_and_stopped() {
        assert_eq!(DaemonStatus::from_flags(true, false), DaemonStatus::Running);
        assert_eq!(DaemonStatus::from_flags(true, true), DaemonStatus::Paused);
        assert_eq!(
            DaemonStatus::from_flags(false, false),
            DaemonStatus::Stopped
        );
    }

    #[test]
    fn transition_states_report_transitioning() {
        assert!(DaemonStatus::Starting.is_transitioning());
        assert!(DaemonStatus::Stopping.is_transitioning());
        assert!(!DaemonStatus::Running.is_transitioning());
    }
}
