use crate::config::MonitorLayout;
use crate::network::protocol::MonitorInfo;

/// Projects a global coordinate (gx, gy) to a client's local screen coordinate (cx, cy) based on the monitor layout and scale factor.
pub fn project_global_to_local(
    gx: i32,
    gy: i32,
    monitor: &MonitorLayout,
    uses_physical_pixels: bool,
) -> (i32, i32) {
    let scale = if uses_physical_pixels { monitor.scale_factor } else { 1.0 };
    let cx = monitor.local_x + ((gx - monitor.x) as f64 * scale) as i32;
    let cy = monitor.local_y + ((gy - monitor.y) as f64 * scale) as i32;
    (cx, cy)
}

/// Projects a local server coordinate (lx, ly) of active_mon to global workspace coordinates.
pub fn project_local_to_global(
    lx: i32,
    ly: i32,
    db_mon: &MonitorLayout,
    active_mon: &MonitorInfo,
) -> (i32, i32) {
    let gx = db_mon.x + (lx - active_mon.local_x);
    let gy = db_mon.y + (ly - active_mon.local_y);
    (gx, gy)
}

/// Snaps global coordinates (gx, gy) to the closest position within the bounds of a target monitor.
pub fn clamp_to_monitor(gx: i32, gy: i32, monitor: &MonitorLayout) -> (i32, i32) {
    let clamped_x = gx.clamp(monitor.x, monitor.x + monitor.width - 1);
    let clamped_y = gy.clamp(monitor.y, monitor.y + monitor.height - 1);
    (clamped_x, clamped_y)
}

/// Finds which client monitor layout contains the specified global coordinate.
pub fn find_client_monitor_containing(gx: i32, gy: i32, layouts: &[MonitorLayout]) -> Option<MonitorLayout> {
    for lay in layouts {
        if lay.host != "main" {
            if gx >= lay.x && gx < lay.x + lay.width
                && gy >= lay.y && gy < lay.y + lay.height {
                return Some(lay.clone());
            }
        }
    }
    None
}

/// Finds the closest client monitor layout corresponding to the given host from a global coordinate.
pub fn find_closest_client_monitor(
    gx: i32,
    gy: i32,
    host: &str,
    monitors: &[MonitorLayout],
) -> Option<MonitorLayout> {
    monitors
        .iter()
        .filter(|m| m.host == host)
        .cloned()
        .min_by_key(|m| {
            let dx = (m.x - gx).max(0).max(gx - (m.x + m.width - 1));
            let dy = (m.y - gy).max(0).max(gy - (m.y + m.height - 1));
            dx * dx + dy * dy
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_global_to_local_no_scale() {
        let monitor = MonitorLayout {
            monitor_id: "client1_disp0".to_string(),
            host: "192.168.1.10".to_string(),
            monitor_name: "Display 0".to_string(),
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 2.0,
            local_x: 0,
            local_y: 0,
        };

        // Without physical pixels (scale = 1.0)
        let local_pos = project_global_to_local(2000, 100, &monitor, false);
        assert_eq!(local_pos, (80, 100));

        // With physical pixels (scale = 2.0)
        let local_pos_scaled = project_global_to_local(2000, 100, &monitor, true);
        assert_eq!(local_pos_scaled, (160, 200));
    }

    #[test]
    fn test_project_local_to_global() {
        let db_mon = MonitorLayout {
            monitor_id: "main_disp0".to_string(),
            host: "main".to_string(),
            monitor_name: "Display 0".to_string(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
            local_x: 100,
            local_y: 100,
        };

        let active_mon = MonitorInfo {
            name: "Display 0".to_string(),
            local_x: 100,
            local_y: 100,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        };

        let global_pos = project_local_to_global(150, 250, &db_mon, &active_mon);
        assert_eq!(global_pos, (50, 150));
    }

    #[test]
    fn test_find_client_monitor_containing() {
        let layouts = vec![
            MonitorLayout {
                monitor_id: "main_disp".to_string(),
                host: "main".to_string(),
                monitor_name: "Disp".to_string(),
                x: 0, y: 0, width: 100, height: 100,
                scale_factor: 1.0, local_x: 0, local_y: 0,
            },
            MonitorLayout {
                monitor_id: "client_disp".to_string(),
                host: "client".to_string(),
                monitor_name: "Disp".to_string(),
                x: 100, y: 0, width: 100, height: 100,
                scale_factor: 1.0, local_x: 0, local_y: 0,
            },
        ];

        // Coordinate falls inside client layout
        let found = find_client_monitor_containing(150, 50, &layouts);
        assert!(found.is_some());
        assert_eq!(found.unwrap().host, "client");

        // Coordinate falls inside server layout (ignored by find_client_monitor_containing)
        let found_server = find_client_monitor_containing(50, 50, &layouts);
        assert!(found_server.is_none());
    }
}
