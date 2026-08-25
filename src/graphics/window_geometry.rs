use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

pub(crate) const DEBUGGER_MIN_SIZE: [u32; 2] = [640, 480];
pub(crate) const DEBUGGER_DEFAULT_SIZE: [u32; 2] = [1100, 760];
pub(crate) const SETTINGS_MIN_SIZE: [u32; 2] = [380, 320];
pub(crate) const SETTINGS_DEFAULT_SIZE: [u32; 2] = [760, 680];
pub(crate) const MODS_MIN_SIZE: [u32; 2] = [420, 320];
pub(crate) const MODS_DEFAULT_SIZE: [u32; 2] = [620, 520];
pub(crate) const CHEATS_MIN_SIZE: [u32; 2] = [480, 360];
pub(crate) const CHEATS_DEFAULT_SIZE: [u32; 2] = [700, 640];
pub(crate) const PRINTER_MIN_SIZE: [u32; 2] = [300, 320];
pub(crate) const PRINTER_DEFAULT_SIZE: [u32; 2] = [520, 720];

pub(crate) fn restored_size(
    saved: [u32; 2],
    minimum: [u32; 2],
    fallback: [u32; 2],
) -> PhysicalSize<u32> {
    let [width, height] = if valid_size(saved, minimum) {
        saved
    } else {
        fallback
    };
    PhysicalSize::new(width, height)
}

pub(crate) fn restored_position(
    event_loop: &ActiveEventLoop,
    position: Option<[i32; 2]>,
    size: PhysicalSize<u32>,
) -> Option<PhysicalPosition<i32>> {
    let position = position?;
    let monitors: Vec<_> = event_loop
        .available_monitors()
        .map(|monitor| {
            let origin = monitor.position();
            let size = monitor.size();
            ([origin.x, origin.y], [size.width, size.height])
        })
        .collect();
    position_visible(position, [size.width, size.height], &monitors)
        .then(|| PhysicalPosition::new(position[0], position[1]))
}

pub(crate) fn can_persist_size(
    window: &Window,
    size: PhysicalSize<u32>,
    minimum: [u32; 2],
) -> bool {
    window.is_minimized() != Some(true)
        && !window.is_maximized()
        && valid_size([size.width, size.height], minimum)
}

pub(crate) fn can_persist_position(window: &Window, position: PhysicalPosition<i32>) -> bool {
    window.is_minimized() != Some(true)
        && !window.is_maximized()
        && position.x > -30_000
        && position.y > -30_000
}

fn valid_size(size: [u32; 2], minimum: [u32; 2]) -> bool {
    size[0] >= minimum[0] && size[1] >= minimum[1]
}

fn position_visible(position: [i32; 2], size: [u32; 2], monitors: &[([i32; 2], [u32; 2])]) -> bool {
    if position[0] <= -30_000 || position[1] <= -30_000 {
        return false;
    }

    let left = i64::from(position[0]);
    let top = i64::from(position[1]);
    let right = left + i64::from(size[0]);

    monitors.iter().any(|&(origin, monitor_size)| {
        let monitor_left = i64::from(origin[0]);
        let monitor_top = i64::from(origin[1]);
        let monitor_right = monitor_left + i64::from(monitor_size[0]);
        let monitor_bottom = monitor_top + i64::from(monitor_size[1]);
        let horizontal_overlap = right.min(monitor_right) - left.max(monitor_left);
        let title_overlap = (top + 32).min(monitor_bottom) - top.max(monitor_top);
        horizontal_overlap >= 64 && title_overlap > 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITORS: &[([i32; 2], [u32; 2])] = &[
        ([-1920, 0], [1920, 1080]),
        ([0, 0], [2560, 1440]),
        ([2560, -400], [1920, 1080]),
    ];

    #[test]
    fn minimized_geometry_uses_defaults() {
        assert_eq!(
            restored_size([1, 1], DEBUGGER_MIN_SIZE, DEBUGGER_DEFAULT_SIZE),
            PhysicalSize::new(1100, 760)
        );
        assert!(!position_visible([-32_000, -32_000], [1100, 760], MONITORS));
    }

    #[test]
    fn positions_on_any_monitor_are_kept() {
        assert!(position_visible([-1600, 100], [1100, 760], MONITORS));
        assert!(position_visible([3000, -300], [1100, 760], MONITORS));
        assert!(!position_visible([5000, 100], [1100, 760], MONITORS));
    }
}
