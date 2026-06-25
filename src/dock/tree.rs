/// Identifier for a tab. Just a `String` for Phase 6 — easy to author by
/// hand and easy to match in the user callback.
pub type TabId = String;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitDirection {
    /// Children sit side-by-side; ratio is the fraction of horizontal
    /// space the first child gets.
    Horizontal,
    /// Children are stacked vertically; ratio is the fraction of vertical
    /// space the first child gets.
    Vertical,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub enum DockNode {
    Split(Split),
    Leaf(Leaf),
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub struct Split {
    pub direction: SplitDirection,
    pub ratio: f32,
    pub first: Box<DockNode>,
    pub second: Box<DockNode>,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub struct Leaf {
    pub tabs: Vec<TabId>,
    pub active: usize,
}

impl DockNode {
    pub fn leaf(tab: impl Into<TabId>) -> Self {
        DockNode::Leaf(Leaf {
            tabs: vec![tab.into()],
            active: 0,
        })
    }

    pub fn tabs(tabs: impl IntoIterator<Item = TabId>) -> Self {
        DockNode::Leaf(Leaf {
            tabs: tabs.into_iter().collect(),
            active: 0,
        })
    }

    pub fn split_h(ratio: f32, first: DockNode, second: DockNode) -> Self {
        DockNode::Split(Split {
            direction: SplitDirection::Horizontal,
            ratio: ratio.clamp(0.05, 0.95),
            first: Box::new(first),
            second: Box::new(second),
        })
    }

    pub fn split_v(ratio: f32, first: DockNode, second: DockNode) -> Self {
        DockNode::Split(Split {
            direction: SplitDirection::Vertical,
            ratio: ratio.clamp(0.05, 0.95),
            first: Box::new(first),
            second: Box::new(second),
        })
    }

    /// Append every tab id in the tree to `out`, in left-to-right traversal
    /// order. Lets a host build a "Window" menu of what's currently docked.
    pub fn collect_tabs(&self, out: &mut Vec<TabId>) {
        match self {
            DockNode::Leaf(leaf) => out.extend(leaf.tabs.iter().cloned()),
            DockNode::Split(split) => {
                split.first.collect_tabs(out);
                split.second.collect_tabs(out);
            }
        }
    }

    /// True if `tab` appears anywhere in the tree.
    pub fn contains_tab(&self, tab: &str) -> bool {
        match self {
            DockNode::Leaf(leaf) => leaf.tabs.iter().any(|t| t == tab),
            DockNode::Split(split) => {
                split.first.contains_tab(tab) || split.second.contains_tab(tab)
            }
        }
    }

    /// Re-open a previously-closed panel by adding it as the active tab of the
    /// least-crowded leaf (fewest tabs), so reopened panels spread out instead
    /// of piling into one narrow column. No-op if it's already present. If the
    /// tree is a single empty leaf, the tab becomes its sole content.
    pub fn add_tab(&mut self, tab: impl Into<TabId>) {
        let tab = tab.into();
        if self.contains_tab(&tab) {
            return;
        }
        match self.smallest_leaf_mut() {
            Some(leaf) => {
                leaf.tabs.push(tab);
                leaf.active = leaf.tabs.len() - 1;
            }
            None => *self = DockNode::leaf(tab),
        }
    }

    /// Make `tab` the active (front) tab of whichever leaf holds it, so
    /// re-opening an already-docked panel surfaces it instead of doing
    /// nothing. Returns `true` if the tab was found.
    pub fn activate_tab(&mut self, tab: &str) -> bool {
        match self {
            DockNode::Leaf(leaf) => {
                if let Some(i) = leaf.tabs.iter().position(|t| t == tab) {
                    leaf.active = i;
                    true
                } else {
                    false
                }
            }
            DockNode::Split(split) => {
                split.first.activate_tab(tab) || split.second.activate_tab(tab)
            }
        }
    }

    /// The leaf with the fewest tabs (ties broken left-to-right), or `None` if
    /// the tree somehow has no leaves.
    fn smallest_leaf_mut(&mut self) -> Option<&mut Leaf> {
        match self {
            DockNode::Leaf(leaf) => Some(leaf),
            DockNode::Split(split) => {
                let a = split.first.smallest_leaf_mut();
                let b = split.second.smallest_leaf_mut();
                match (a, b) {
                    (Some(a), Some(b)) => {
                        if b.tabs.len() < a.tabs.len() {
                            Some(b)
                        } else {
                            Some(a)
                        }
                    }
                    (a, b) => a.or(b),
                }
            }
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    #[test]
    fn dock_tree_json_round_trip() {
        let tree = DockNode::split_h(
            0.25,
            DockNode::leaf("Hierarchy"),
            DockNode::tabs(vec!["Console".into(), "Profiler".into()]),
        );
        let json = serde_json::to_string(&tree).unwrap();
        let back: DockNode = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_string(&back).unwrap(),
            json,
            "tree should survive a JSON round trip unchanged"
        );
    }
}
