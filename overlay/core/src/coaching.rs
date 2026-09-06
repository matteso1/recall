//! Short, deterministic explanations. The planner picks the action; this module
//! explains it without inventing mechanics, player intent, or win probability.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    #[default]
    Core,
    Completion,
    ArmorPen,
    MagicPen,
    AntiHeal,
    Cleanse,
    MagicDefense,
    PhysicalDefense,
    AntiBurst,
    Sustain,
    Boots,
    Start,
    Pinned,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    #[default]
    Aggregate,
    VisibleItems,
    Composition,
    Inventory,
    PlayerChoice,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct LearningTip {
    pub kind: DecisionKind,
    /// The only explanation shown in the main view.
    pub reason: String,
    pub evidence: Evidence,
    pub title: String,
    /// Extra teaching is optional, never a quiz or a prerequisite for a purchase.
    pub lesson: String,
    pub tradeoff: String,
}

pub fn explain(kind: DecisionKind, reason: String, evidence: Evidence) -> LearningTip {
    use DecisionKind::*;
    let (title, lesson, tradeoff) = match kind {
        Core => (
            "A starting point, not a guarantee",
            "A popular build is useful evidence of what fits this champion. Its win rate also reflects who bought it and which games lasted long enough.",
            "Keeps the common build order until a concrete purchase or matchup need justifies changing it.",
        ),
        Completion => (
            "Compare the gold left to spend",
            "Owned components reduce the price of their upgrades. A nearly finished item can be the cheaper immediate spike even when its full price is higher.",
            "Finishes existing investment before starting another item; this does not prove the highest possible damage.",
        ),
        ArmorPen => (
            "Armor changes the value of damage",
            "Percentage armor penetration bypasses part of a target's armor. More visible armor makes that effect more relevant to physical damage.",
            "Brings penetration forward at the cost of delaying another damage or defensive item.",
        ),
        MagicPen => (
            "Match penetration to the resistance",
            "Magic penetration helps magic damage against magic resistance. It is not a substitute for armor penetration on physical damage.",
            "Brings magic penetration forward at the cost of delaying another item.",
        ),
        AntiHeal => (
            "Reduce healing when you can apply it",
            "Grievous Wounds reduces healing while applied and does not stack with another copy. An ally owning anti-heal does not guarantee coverage on your target.",
            "A small anti-heal detour delays your next full item. Return to the main build after buying it.",
        ),
        Cleanse => (
            "The active is the reason to buy it",
            "Quicksilver removes eligible crowd control, including suppression, but must be activated. It does not erase the damage or remove airborne effects. Summoner Cleanse cannot remove suppression.",
            "Buys an escape from verified crowd control instead of immediate damage; it is not automatic protection.",
        ),
        MagicDefense => (
            "Defend against the relevant damage",
            "Magic resistance reduces magic damage. Visible equipment helps identify a relevant threat, but item value is not the enemy's total gold or exact damage.",
            "Trades some offensive progress for magic protection.",
        ),
        PhysicalDefense => (
            "Protection can make your damage usable",
            "Armor reduces physical damage. Staying alive long enough to contribute can matter more than another purely offensive purchase.",
            "Trades some offensive progress for physical protection.",
        ),
        AntiBurst => (
            "A buffer against burst",
            "A shield or another defensive effect can buy time against a quick burst. It does not guarantee survival, and active items still need your input.",
            "Prioritizes a defensive buffer over a pure damage upgrade.",
        ),
        Sustain => (
            "Recovery is different from surviving burst",
            "Sustain helps recover health during or between trades when you can safely apply it. It is less reliable when you die before getting those attacks or spells off.",
            "Favors repeatable recovery over a stronger immediate damage or burst-defense purchase.",
        ),
        Boots => (
            "Movement is part of the build",
            "Boots add movement speed and a specialized benefit. One pair is enough; components you already own count toward its upgrade.",
            "Spends some gold on movement and utility before the next full item.",
        ),
        Start => (
            "Start with a usable kit",
            "Opening items provide early stats, sustain, or your role's quest. Items you already bought count toward the kit; it does not restart every recall.",
            "Uses starting gold on early needs before a large upgrade; no implicit sale is part of this recommendation.",
        ),
        Pinned => (
            "Your target, a legal purchase path",
            "This target is pinned by you. The panel still checks recipes, remaining gold, inventory space, and incompatible items. Return to Auto to let it choose again.",
            "Keeps your chosen target even when the automatic planner prefers another item.",
        ),
    };
    LearningTip {
        kind,
        reason,
        evidence,
        title: title.into(),
        lesson: lesson.into(),
        tradeoff: tradeoff.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explanation_preserves_the_actual_decision_instead_of_choosing_an_item() {
        let reason = "IE: only 725g left with your components";
        let tip = explain(DecisionKind::Completion, reason.into(), Evidence::Inventory);
        assert_eq!(tip.reason, reason);
        assert!(tip.lesson.contains("Owned components"));
        assert!(!tip.lesson.contains("win chance"));
    }

    #[test]
    fn cleanse_lesson_distinguishes_suppression_damage_and_airborne() {
        let tip = explain(
            DecisionKind::Cleanse,
            "QSS for Nether Grasp".into(),
            Evidence::Composition,
        );
        assert!(tip
            .lesson
            .contains("Summoner Cleanse cannot remove suppression"));
        assert!(tip.lesson.contains("must be activated"));
        assert!(tip.lesson.contains("damage") && tip.lesson.contains("airborne"));
    }
}
