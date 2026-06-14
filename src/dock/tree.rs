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
