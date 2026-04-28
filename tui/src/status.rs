use ratatui::style::{Color, Style};
use taurine_core::rpc::{
    DEFAULT_RPC_URL, StatusRequest, daemon_control_client::DaemonControlClient,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DaemonStatus {
    Running,
    #[default]
    Stopped,
    Idle,
}

impl DaemonStatus {
    pub(crate) const fn from_flags(online: bool, paused: bool) -> Self {
        if !online {
            Self::Stopped
        } else if paused {
            Self::Idle
        } else {
            Self::Running
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Idle => "idle",
        }
    }

    pub(crate) const fn color(self) -> Color {
        match self {
            Self::Running => Color::Green,
            Self::Stopped => Color::Red,
            Self::Idle => Color::Yellow,
        }
    }

    pub(crate) fn style(self) -> Style {
        Style::default().fg(self.color())
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
        let Ok(mut client) = DaemonControlClient::connect(DEFAULT_RPC_URL).await else {
            return DaemonStatus::Stopped;
        };

        let request = tonic::Request::new(StatusRequest {});
        let Ok(response) = client.get_status(request).await else {
            return DaemonStatus::Stopped;
        };

        let status = response.into_inner();
        DaemonStatus::from_flags(status.online, status.paused)
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
    fn idle_label_is_lowercase_idle() {
        assert_eq!(DaemonStatus::Idle.label(), "idle");
    }

    #[test]
    fn status_color_mapping_matches_design() {
        assert_eq!(DaemonStatus::Running.color(), Color::Green);
        assert_eq!(DaemonStatus::Stopped.color(), Color::Red);
        assert_eq!(DaemonStatus::Idle.color(), Color::Yellow);
    }

    #[test]
    fn status_flags_map_to_running_idle_and_stopped() {
        assert_eq!(DaemonStatus::from_flags(true, false), DaemonStatus::Running);
        assert_eq!(DaemonStatus::from_flags(true, true), DaemonStatus::Idle);
        assert_eq!(
            DaemonStatus::from_flags(false, false),
            DaemonStatus::Stopped
        );
    }
}
