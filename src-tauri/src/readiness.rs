#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Unknown,
    Granted,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadinessIssue {
    pub code: &'static str,
    pub message: &'static str,
    pub guidance: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessOutcome {
    pub recording_ready: bool,
    pub shortcut_ready: bool,
    pub issues: Vec<ReadinessIssue>,
}

pub fn permission_state_from_granted(granted: bool) -> PermissionState {
    if granted {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    }
}

pub fn evaluate_readiness(
    mic_permission: PermissionState,
    input_monitoring_permission: PermissionState,
    shortcut_registered: bool,
) -> ReadinessOutcome {
    let mut issues = Vec::new();

    if mic_permission == PermissionState::Denied {
        issues.push(mic_permission_denied_issue());
    }

    if input_monitoring_permission == PermissionState::Denied {
        issues.push(input_monitoring_permission_denied_issue());
    }

    ReadinessOutcome {
        recording_ready: mic_permission != PermissionState::Denied,
        shortcut_ready: shortcut_registered
            && input_monitoring_permission != PermissionState::Denied,
        issues,
    }
}

pub fn ensure_recording_start_allowed(
    mic_permission: PermissionState,
) -> Result<(), ReadinessIssue> {
    if mic_permission == PermissionState::Denied {
        return Err(mic_permission_denied_issue());
    }

    Ok(())
}

pub fn ensure_shortcut_registration_allowed(
    input_monitoring_permission: PermissionState,
) -> Result<(), ReadinessIssue> {
    if input_monitoring_permission == PermissionState::Denied {
        return Err(input_monitoring_permission_denied_issue());
    }

    Ok(())
}

pub fn mic_permission_denied_issue() -> ReadinessIssue {
    ReadinessIssue {
        code: "mic_permission_denied",
        message: "Microphone permission is denied. Grant permission and retry.",
        guidance: "Enable Microphone access in macOS Privacy settings.",
    }
}

pub fn input_monitoring_permission_denied_issue() -> ReadinessIssue {
    ReadinessIssue {
        code: "input_monitoring_permission_denied",
        message: "Input Monitoring permission is required for global shortcut capture.",
        guidance: "Enable Input Monitoring and Accessibility permissions.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denied_mic_permission_blocks_recording_start() {
        let result = ensure_recording_start_allowed(PermissionState::Denied);
        assert!(result.is_err());
        let issue = result.expect_err("expected mic readiness issue");
        assert_eq!(issue.code, "mic_permission_denied");
    }

    #[test]
    fn denied_input_monitoring_blocks_shortcut_registration() {
        let result = ensure_shortcut_registration_allowed(PermissionState::Denied);
        assert!(result.is_err());
        let issue = result.expect_err("expected input readiness issue");
        assert_eq!(issue.code, "input_monitoring_permission_denied");
    }
}
