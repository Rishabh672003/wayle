use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

use super::helpers::{is_special_workspace, matches_ignore_patterns};

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceData {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub windows: u16,
    pub monitor: String,
}

#[derive(Debug, Clone)]
pub(crate) struct FilterContext<'a> {
    pub show_special: bool,
    pub monitor_specific: bool,
    pub min_workspace_count: usize,
    pub active_workspace_id: String,
    pub bar_monitor: Option<&'a str>,
    pub ignore_patterns: &'a [String],
    pub workspace_monitor_rules: &'a HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FilteredWorkspace {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub windows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SortKey {
    is_numeric: bool,
    number: i64,
    label: String,
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.is_numeric, other.is_numeric) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => self.number.cmp(&other.number),
            (false, false) => self.label.cmp(&other.label),
        }
    }
}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn sort_key(id: &str) -> SortKey {
    match id.parse::<i64>() {
        Ok(number) => SortKey {
            is_numeric: true,
            number,
            label: String::new(),
        },
        Err(_) => SortKey {
            is_numeric: false,
            number: 0,
            label: id.to_string(),
        },
    }
}

pub(crate) fn filter_workspaces(
    workspaces: &[WorkspaceData],
    ctx: &FilterContext<'_>,
) -> Vec<FilteredWorkspace> {
    let max_id = ctx.min_workspace_count as i64;

    let mut filtered: Vec<FilteredWorkspace> = workspaces
        .iter()
        .filter(|ws| should_include_workspace(ws, ctx, max_id))
        .map(|ws| FilteredWorkspace {
            id: ws.id.clone(),
            kind: ws.kind.clone(),
            name: ws.name.clone(),
            windows: ws.windows,
        })
        .collect();

    filtered.sort_by_key(|a| sort_key(&a.id));

    if ctx.min_workspace_count > 0 {
        add_placeholder_workspaces(&mut filtered, workspaces, ctx, max_id);
    }

    filtered.sort_by_key(|a| sort_key(&a.id));
    filtered
}

fn should_include_workspace(ws: &WorkspaceData, ctx: &FilterContext<'_>, max_id: i64) -> bool {
    if matches_ignore_patterns(&ws.id, ctx.ignore_patterns) {
        return false;
    }

    if is_special_workspace(&ws.id, &ws.kind) && !ctx.show_special {
        return false;
    }

    if exceeds_min_count_limit(
        &ws.id,
        ws.windows,
        max_id,
        &ctx.active_workspace_id,
        ctx.min_workspace_count,
    ) {
        return false;
    }

    if belongs_to_different_monitor(&ws.monitor, ctx.monitor_specific, ctx.bar_monitor) {
        return false;
    }

    true
}

fn exceeds_min_count_limit(
    id: &str,
    windows: u16,
    max_id: i64,
    active_id: &str,
    min_count: usize,
) -> bool {
    let has_limit = min_count > 0;
    let is_normal = id.parse::<i64>().is_ok_and(|value| value > 0);
    let beyond_limit = id.parse::<i64>().is_ok_and(|value| value > max_id);
    let is_active = id == active_id;
    let is_occupied = windows > 0;

    has_limit && is_normal && beyond_limit && !is_active && !is_occupied
}

fn belongs_to_different_monitor(
    workspace_monitor: &str,
    monitor_specific: bool,
    bar_monitor: Option<&str>,
) -> bool {
    if !monitor_specific {
        return false;
    }
    let Some(bar_mon) = bar_monitor else {
        return false;
    };
    workspace_monitor != bar_mon
}

fn add_placeholder_workspaces(
    filtered: &mut Vec<FilteredWorkspace>,
    all_workspaces: &[WorkspaceData],
    ctx: &FilterContext<'_>,
    max_id: i64,
) {
    let mut existing_ids: HashSet<String> = filtered
        .iter()
        .map(|ws| ws.id.clone())
        .chain(all_workspaces.iter().map(|ws| ws.id.clone()).filter(|id| {
            id.parse::<i64>()
                .is_ok_and(|value| value > 0 && value <= max_id)
        }))
        .collect();

    for id in 1..=max_id {
        let id_str = id.to_string();
        if existing_ids.contains(&id_str) {
            continue;
        }

        if matches_ignore_patterns(&id_str, ctx.ignore_patterns) {
            continue;
        }

        if ctx.monitor_specific {
            let Some(bar_monitor) = ctx.bar_monitor else {
                continue;
            };
            let rule_monitor = ctx.workspace_monitor_rules.get(&id_str);
            if rule_monitor.map(String::as_str) != Some(bar_monitor) {
                continue;
            }
        }

        filtered.push(FilteredWorkspace {
            id: id_str.clone(),
            kind: "numbered".to_string(),
            name: String::new(),
            windows: 0,
        });
        existing_ids.insert(id_str);
    }

    if ctx
        .active_workspace_id
        .parse::<i64>()
        .is_ok_and(|value| value > 0)
        && !existing_ids.contains(&ctx.active_workspace_id)
        && !matches_ignore_patterns(&ctx.active_workspace_id, ctx.ignore_patterns)
        && should_include_active_workspace_placeholder(all_workspaces, ctx)
    {
        filtered.push(FilteredWorkspace {
            id: ctx.active_workspace_id.clone(),
            kind: "numbered".to_string(),
            name: String::new(),
            windows: 0,
        });
    }
}

fn should_include_active_workspace_placeholder(
    all_workspaces: &[WorkspaceData],
    ctx: &FilterContext<'_>,
) -> bool {
    if !ctx.monitor_specific {
        return true;
    }

    let Some(bar_monitor) = ctx.bar_monitor else {
        return true;
    };

    let active_monitor = all_workspaces
        .iter()
        .find(|ws| ws.id == ctx.active_workspace_id)
        .map(|ws| ws.monitor.as_str())
        .or_else(|| {
            ctx.workspace_monitor_rules
                .get(&ctx.active_workspace_id)
                .map(String::as_str)
        });

    !matches!(active_monitor, Some(monitor) if monitor != bar_monitor)
}

pub(crate) fn monitor_workspaces_sorted(
    bar_monitor: &str,
    workspace_monitor_rules: &HashMap<String, String>,
) -> Vec<String> {
    let mut matching: Vec<_> = workspace_monitor_rules
        .iter()
        .filter(|(_, monitor)| monitor.as_str() == bar_monitor)
        .map(|(id, _)| id.clone())
        .filter(|id| id.parse::<i64>().is_ok_and(|value| value > 0))
        .collect();

    matching.sort_by_key(|a| sort_key(a));
    matching
}

pub(crate) fn relative_workspace_number(id: &str, monitor_workspaces: &[String]) -> String {
    if monitor_workspaces.is_empty() {
        return id.to_string();
    }

    monitor_workspaces
        .iter()
        .position(|ws_id| ws_id == id)
        .map(|pos| (pos + 1).to_string())
        .unwrap_or_else(|| id.to_string())
}

pub(crate) fn calculate_navigation_index(
    current_idx: usize,
    direction: i64,
    total: usize,
) -> usize {
    if direction > 0 {
        (current_idx + 1) % total
    } else if current_idx == 0 {
        total - 1
    } else {
        current_idx - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod filter_workspaces {
        use super::*;

        fn make_workspace(id: &str, monitor: &str) -> WorkspaceData {
            WorkspaceData {
                id: id.to_string(),
                kind: "numbered".to_string(),
                name: id.to_string(),
                windows: 1,
                monitor: monitor.to_string(),
            }
        }

        fn make_empty_workspace(id: &str, monitor: &str) -> WorkspaceData {
            WorkspaceData {
                id: id.to_string(),
                kind: "numbered".to_string(),
                name: id.to_string(),
                windows: 0,
                monitor: monitor.to_string(),
            }
        }

        fn make_special_workspace(id: &str, kind: &str, monitor: &str) -> WorkspaceData {
            WorkspaceData {
                id: id.to_string(),
                kind: kind.to_string(),
                name: id.to_string(),
                windows: 0,
                monitor: monitor.to_string(),
            }
        }

        #[test]
        fn filters_by_monitor_when_monitor_specific() {
            let workspaces = vec![
                make_workspace("1", "DP-1"),
                make_workspace("2", "DP-2"),
                make_workspace("3", "DP-1"),
            ];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: true,
                min_workspace_count: 0,
                active_workspace_id: "1".to_string(),
                bar_monitor: Some("DP-1"),
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert_eq!(ids, vec!["1", "3"]);
        }

        #[test]
        fn excludes_special_workspaces_by_default() {
            let workspaces = vec![
                make_workspace("1", "DP-1"),
                make_special_workspace("special:magic", "special", "DP-1"),
            ];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 0,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert_eq!(ids, vec!["1"]);
        }

        #[test]
        fn excludes_legacy_special_negative_ids_by_default() {
            let workspaces = vec![
                make_workspace("1", "DP-1"),
                make_special_workspace("-99", "special", "DP-1"),
            ];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 0,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert_eq!(ids, vec!["1"]);
        }

        #[test]
        fn includes_special_when_enabled() {
            let workspaces = vec![
                make_workspace("1", "DP-1"),
                make_special_workspace("special:magic", "special", "DP-1"),
            ];

            let ctx = FilterContext {
                show_special: true,
                monitor_specific: false,
                min_workspace_count: 0,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert_eq!(ids, vec!["1", "special:magic"]);
        }

        #[test]
        fn sorts_numeric_before_named() {
            let workspaces = vec![
                make_workspace("web", "DP-1"),
                make_workspace("2", "DP-1"),
                make_workspace("10", "DP-1"),
                make_workspace("1", "DP-1"),
            ];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 0,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert_eq!(ids, vec!["1", "2", "10", "web"]);
        }

        #[test]
        fn respects_ignore_patterns() {
            let workspaces = vec![
                make_workspace("1", "DP-1"),
                make_workspace("2", "DP-1"),
                make_workspace("10", "DP-1"),
            ];

            let patterns = vec!["10".to_string()];
            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 0,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &patterns,
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert_eq!(ids, vec!["1", "2"]);
        }

        #[test]
        fn adds_placeholder_workspaces_up_to_min_count() {
            let workspaces = vec![make_workspace("1", "DP-1")];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 3,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert_eq!(ids, vec!["1", "2", "3"]);
        }

        #[test]
        fn always_includes_active_workspace() {
            let workspaces = vec![make_workspace("1", "DP-1")];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 3,
                active_workspace_id: "5".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert!(ids.contains(&"5"));
        }

        #[test]
        fn includes_occupied_workspace_beyond_min_count_limit() {
            let workspaces = vec![make_workspace("1", "DP-1"), make_workspace("9", "DP-1")];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 8,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert!(ids.contains(&"9"));
        }

        #[test]
        fn excludes_empty_workspace_beyond_min_count_limit_when_not_active() {
            let workspaces = vec![
                make_workspace("1", "DP-1"),
                make_empty_workspace("9", "DP-1"),
            ];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 8,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert!(!ids.contains(&"9"));
        }

        #[test]
        fn monitor_specific_does_not_readd_active_workspace_from_other_monitor() {
            let workspaces = vec![make_workspace("1", "DP-1"), make_workspace("9", "DP-2")];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: true,
                min_workspace_count: 8,
                active_workspace_id: "9".to_string(),
                bar_monitor: Some("DP-1"),
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert!(!ids.contains(&"9"));
        }

        #[test]
        fn monitor_specific_does_not_readd_active_workspace_from_other_monitor_rule() {
            let workspaces = vec![make_workspace("1", "DP-1")];
            let mut rules = HashMap::new();
            rules.insert("9".to_string(), "DP-2".to_string());

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: true,
                min_workspace_count: 8,
                active_workspace_id: "9".to_string(),
                bar_monitor: Some("DP-1"),
                ignore_patterns: &[],
                workspace_monitor_rules: &rules,
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert!(!ids.contains(&"9"));
        }

        #[test]
        fn monitor_specific_includes_occupied_workspace_beyond_min_count_on_bar_monitor() {
            let workspaces = vec![make_workspace("1", "DP-1"), make_workspace("9", "DP-1")];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: true,
                min_workspace_count: 8,
                active_workspace_id: "1".to_string(),
                bar_monitor: Some("DP-1"),
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert!(ids.contains(&"9"));
        }

        #[test]
        fn monitor_specific_excludes_occupied_workspace_beyond_min_count_on_other_monitor() {
            let workspaces = vec![make_workspace("1", "DP-1"), make_workspace("9", "DP-2")];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: true,
                min_workspace_count: 8,
                active_workspace_id: "1".to_string(),
                bar_monitor: Some("DP-1"),
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert!(!ids.contains(&"9"));
        }

        #[test]
        fn monitor_specific_readds_active_workspace_placeholder_when_rule_matches_bar_monitor() {
            let workspaces = vec![make_workspace("1", "DP-1")];
            let mut rules = HashMap::new();
            rules.insert("9".to_string(), "DP-1".to_string());

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: true,
                min_workspace_count: 8,
                active_workspace_id: "9".to_string(),
                bar_monitor: Some("DP-1"),
                ignore_patterns: &[],
                workspace_monitor_rules: &rules,
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert!(ids.contains(&"9"));
        }

        #[test]
        fn monitor_specific_placeholders_require_explicit_rules() {
            let workspaces = vec![make_workspace("1", "DP-1")];

            let mut rules = HashMap::new();
            rules.insert("1".to_string(), "DP-1".to_string());
            rules.insert("3".to_string(), "DP-1".to_string());

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: true,
                min_workspace_count: 5,
                active_workspace_id: "1".to_string(),
                bar_monitor: Some("DP-1"),
                ignore_patterns: &[],
                workspace_monitor_rules: &rules,
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();

            assert!(ids.contains(&"1"));
            assert!(ids.contains(&"3"));
            assert!(!ids.contains(&"2"));
            assert!(!ids.contains(&"4"));
            assert!(!ids.contains(&"5"));
        }

        #[test]
        fn global_mode_adds_all_placeholders() {
            let workspaces = vec![make_workspace("1", "DP-1")];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 5,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();

            assert_eq!(ids, vec!["1", "2", "3", "4", "5"]);
        }

        #[test]
        fn placeholders_respect_ignore_patterns() {
            let workspaces = vec![make_workspace("1", "DP-1")];
            let patterns = vec!["3".to_string(), "5".to_string()];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 5,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &patterns,
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();

            assert_eq!(ids, vec!["1", "2", "4"]);
        }

        #[test]
        fn active_workspace_respects_ignore_patterns() {
            let workspaces = vec![make_workspace("1", "DP-1")];
            let patterns = vec!["10".to_string()];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 5,
                active_workspace_id: "10".to_string(),
                bar_monitor: None,
                ignore_patterns: &patterns,
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();

            assert!(!ids.contains(&"10"));
        }

        #[test]
        fn named_workspace_not_subject_to_min_count_limit() {
            let workspaces = vec![
                make_workspace("1", "DP-1"),
                WorkspaceData {
                    id: "web".to_string(),
                    kind: "named".to_string(),
                    name: "web".to_string(),
                    windows: 0,
                    monitor: "DP-1".to_string(),
                },
            ];

            let ctx = FilterContext {
                show_special: false,
                monitor_specific: false,
                min_workspace_count: 1,
                active_workspace_id: "1".to_string(),
                bar_monitor: None,
                ignore_patterns: &[],
                workspace_monitor_rules: &HashMap::new(),
            };

            let result = filter_workspaces(&workspaces, &ctx);
            let ids: Vec<_> = result.iter().map(|ws| ws.id.as_str()).collect();
            assert!(ids.contains(&"web"));
        }
    }

    mod relative_workspace_number {
        use super::*;

        #[test]
        fn returns_id_when_empty_list() {
            assert_eq!(relative_workspace_number("5", &[]), "5");
        }

        #[test]
        fn returns_position_plus_one() {
            let workspaces = vec!["4".to_string(), "5".to_string(), "6".to_string()];
            assert_eq!(relative_workspace_number("5", &workspaces), "2");
        }

        #[test]
        fn returns_id_when_not_found() {
            let workspaces = vec!["1".to_string(), "2".to_string(), "3".to_string()];
            assert_eq!(relative_workspace_number("10", &workspaces), "10");
        }
    }

    mod calculate_navigation_index {
        use super::*;

        #[test]
        fn wraps_forward_at_end() {
            assert_eq!(calculate_navigation_index(4, 1, 5), 0);
        }

        #[test]
        fn wraps_backward_at_start() {
            assert_eq!(calculate_navigation_index(0, -1, 5), 4);
        }

        #[test]
        fn moves_forward() {
            assert_eq!(calculate_navigation_index(2, 1, 5), 3);
        }

        #[test]
        fn moves_backward() {
            assert_eq!(calculate_navigation_index(2, -1, 5), 1);
        }
    }

    mod monitor_workspaces_sorted {
        use super::*;

        #[test]
        fn filters_and_sorts() {
            let mut rules = HashMap::new();
            rules.insert("5".to_string(), "DP-1".to_string());
            rules.insert("1".to_string(), "DP-1".to_string());
            rules.insert("3".to_string(), "DP-2".to_string());
            rules.insert("2".to_string(), "DP-1".to_string());

            let result = monitor_workspaces_sorted("DP-1", &rules);
            assert_eq!(result, vec!["1", "2", "5"]);
        }

        #[test]
        fn excludes_negative_ids() {
            let mut rules = HashMap::new();
            rules.insert("1".to_string(), "DP-1".to_string());
            rules.insert("-99".to_string(), "DP-1".to_string());

            let result = monitor_workspaces_sorted("DP-1", &rules);
            assert_eq!(result, vec!["1"]);
        }
    }
}
