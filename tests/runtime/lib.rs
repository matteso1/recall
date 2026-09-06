//! Compiles actual shell planning/polling modules without a GUI or account requests.
#![allow(dead_code)]
extern crate self as tauri;
#[derive(Clone, Default)]
pub struct AppHandle;
#[path = "../../overlay/src-tauri/src/controller.rs"]
mod controller;
#[path = "../../overlay/src-tauri/src/journal_store.rs"]
mod journal_store;
#[path = "../../overlay/src-tauri/src/poller.rs"]
mod poller;
#[path = "../../overlay/src-tauri/src/probe.rs"]
mod probe;
#[path = "../../overlay/src-tauri/src/rune_queue.rs"]
mod rune_queue;
#[path = "../../overlay/src-tauri/src/settings.rs"]
mod settings;
#[path = "../../overlay/src-tauri/src/swiftplay.rs"]
mod swiftplay;

use recall_core::{
    aggregate::Aggregate,
    champselect::Lobby,
    ddragon::{normalize, Catalog},
    engine::{Plan, PlannerPreferences},
    journal::Journal,
    lcu::Lcu,
    live::LiveSnapshot,
    pack::{ChampionPack, Traits},
    state::PanelState,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct AggState {
    pub refresh: recall_core::session::RefreshGate<recall_core::session::AggregateKey>,
    pub value: Option<Arc<Aggregate>>,
    pub error: Option<String>,
    pub task: Option<tokio::task::JoinHandle<()>>,
}
pub struct App {
    pub planning: Mutex<()>,
    pub panel: Mutex<PanelState>,
    pub catalog: Mutex<Option<Arc<Catalog>>>,
    pub pack: ChampionPack,
    pub traits: Traits,
    pub lcu: Mutex<Option<Lcu>>,
    pub lobby: Mutex<Option<Lobby>>,
    pub summoner_id: Mutex<Option<u64>>,
    pub plan: Mutex<Option<Plan>>,
    pub settings: Mutex<settings::Settings>,
    pub aggregate: Mutex<AggState>,
    pub preferences: Mutex<PlannerPreferences>,
    pub latest_live: Mutex<Option<LiveSnapshot>>,
    pub lobby_observed_at_ms: Mutex<Option<u64>>,
    pub session: Mutex<controller::RecommendationSession>,
    pub journal: Mutex<Journal>,
    pub journal_sink: journal_store::JournalSink,
}
impl App {
    pub fn snapshot(&self) -> PanelState {
        self.panel.lock().unwrap().clone()
    }
    pub fn pack_for(&self, champion: &str) -> Option<&ChampionPack> {
        (normalize(champion) == normalize(&self.pack.champion)).then_some(&self.pack)
    }
    pub fn update(&self, _: &AppHandle, f: impl FnOnce(&mut PanelState)) {
        f(&mut self.panel.lock().unwrap());
    }
}
mod commands {
    use super::*;
    pub async fn do_import_item_set_for_plan(
        _: &AppHandle,
        _: &Arc<App>,
        _: Plan,
    ) -> Result<String, String> {
        Err("not called by harness".into())
    }
    pub async fn do_import_runes_for_plan(
        _: &AppHandle,
        _: &Arc<App>,
        _: Plan,
    ) -> Result<String, String> {
        Err("not called by harness".into())
    }
    pub async fn do_import_spells_for_plan(
        _: &AppHandle,
        _: &Arc<App>,
        _: Plan,
    ) -> Result<String, String> {
        Err("not called by harness".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recall_core::{
        aggregate::{self, Position},
        engine::BuildPreference,
        journal::Feedback,
        live::{InvItem, Me, Player},
        session::AggregateKey,
        state::SourceStatus,
    };

    fn app() -> (Arc<App>, AppHandle) {
        let items =
            serde_json::from_str(include_str!("../../m0/tests/fixtures/item_subset.json")).unwrap();
        let champions =
            serde_json::from_str(include_str!("../../m0/tests/fixtures/champion_subset.json"))
                .unwrap();
        let catalog = Arc::new(Catalog::from_json(
            "16.17.1",
            &items,
            &champions,
            &serde_json::json!([]),
        ));
        let raw = serde_json::from_str(include_str!("../../m0/tests/fixtures/opgg_xayah_adc.json"))
            .unwrap();
        let aggregate = Arc::new(
            aggregate::decode(&raw, 498, Position::Adc, "global", "emerald_plus").unwrap(),
        );
        let mut agg = AggState::default();
        agg.refresh.begin(
            AggregateKey {
                champion_key: 498,
                position: Some(Position::Adc),
                region: "global".into(),
                tier: "emerald_plus".into(),
            },
            controller::now_ms(),
        );
        agg.value = Some(aggregate);
        let live = LiveSnapshot {
            game_time: 300.0,
            mode: "CLASSIC".into(),
            me: Some(Me {
                player: Player {
                    champion: "Xayah".into(),
                    position: "BOTTOM".into(),
                    level: 9,
                    ..Default::default()
                },
                gold: 1200.0,
                spell_ids: vec![4, 21],
                rune_ids: Some(vec![]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (journal, sink, _) = journal_store::disabled();
        let st = Arc::new(App {
            planning: Mutex::new(()),
            panel: Mutex::new(PanelState {
                phase: "ingame".into(),
                champion: Some("Xayah".into()),
                supported: true,
                live_source: Some(SourceStatus {
                    observed_at_ms: Some(controller::now_ms()),
                    age_ms: Some(0),
                    stale: false,
                    identity_known: true,
                }),
                ..Default::default()
            }),
            catalog: Mutex::new(Some(catalog.clone())),
            pack: recall_core::pack::load_xayah().unwrap(),
            traits: recall_core::pack::load_traits().unwrap(),
            lcu: Mutex::new(None),
            lobby: Mutex::new(None),
            summoner_id: Mutex::new(None),
            plan: Mutex::new(None),
            settings: Mutex::new(settings::Settings::default()),
            aggregate: Mutex::new(agg),
            preferences: Mutex::new(PlannerPreferences::default()),
            latest_live: Mutex::new(Some(live.clone())),
            lobby_observed_at_ms: Mutex::new(None),
            session: Mutex::new(controller::RecommendationSession::default()),
            journal: Mutex::new(journal),
            journal_sink: sink,
        });
        let handle = AppHandle;
        {
            let _planning = st.planning.lock().unwrap();
            controller::ensure_champion_locked(&handle, &st, Some("Xayah"));
            let aggregate = st.aggregate.lock().unwrap().value.clone();
            let plan = controller::compute_locked(
                &st,
                &catalog,
                "Xayah",
                aggregate.as_deref(),
                &[],
                Some(&live),
            );
            assert!(!plan.path.is_empty());
            controller::store_plan_locked(&st, &catalog, &plan, Some(&live));
            st.update(&handle, |panel| panel.plan = Some(plan));
        }
        (st, handle)
    }

    #[test]
    fn controller_immediately_applies_mode_pin_and_clear_locally() {
        let (st, handle) = app();
        controller::change_preference(
            &handle,
            &st,
            controller::PreferenceChange::Mode(BuildPreference::Survival),
        )
        .unwrap();
        assert_eq!(
            st.snapshot().plan.unwrap().preferences.mode,
            BuildPreference::Survival
        );
        controller::change_preference(&handle, &st, controller::PreferenceChange::Pin(3031))
            .unwrap();
        assert_eq!(st.snapshot().plan.unwrap().next.unwrap().id, 3031);
        assert_eq!(st.preferences.lock().unwrap().pinned_item, Some(3031));
        controller::change_preference(&handle, &st, controller::PreferenceChange::ClearPin)
            .unwrap();
        assert_eq!(st.preferences.lock().unwrap().pinned_item, None);
    }

    #[test]
    fn completed_target_clears_the_persisted_effective_pin() {
        let (st, handle) = app();
        controller::change_preference(&handle, &st, controller::PreferenceChange::Pin(3031))
            .unwrap();
        let _planning = st.planning.lock().unwrap();
        let mut live = st.latest_live.lock().unwrap().clone().unwrap();
        live.game_time += 2.0;
        live.me.as_mut().unwrap().player.items.push(InvItem {
            id: 3031,
            name: "Infinity Edge".into(),
            count: 1,
            slot: 0,
        });
        let catalog = st.catalog.lock().unwrap().clone().unwrap();
        let aggregate = st.aggregate.lock().unwrap().value.clone();
        let plan = controller::compute_locked(
            &st,
            &catalog,
            "Xayah",
            aggregate.as_deref(),
            &[],
            Some(&live),
        );
        controller::store_plan_locked(&st, &catalog, &plan, Some(&live));
        assert_eq!(st.preferences.lock().unwrap().pinned_item, None);
    }

    #[test]
    fn stale_source_or_missing_identity_cannot_pin_or_change_the_plan() {
        let (st, handle) = app();
        st.panel
            .lock()
            .unwrap()
            .live_source
            .as_mut()
            .unwrap()
            .observed_at_ms = Some(controller::now_ms() - 10_000);
        assert!(controller::change_preference(
            &handle,
            &st,
            controller::PreferenceChange::Pin(3031)
        )
        .is_err());
        assert_eq!(st.preferences.lock().unwrap().pinned_item, None);
        st.panel
            .lock()
            .unwrap()
            .live_source
            .as_mut()
            .unwrap()
            .observed_at_ms = Some(controller::now_ms());
        st.latest_live.lock().unwrap().as_mut().unwrap().me = None;
        assert!(controller::change_preference(
            &handle,
            &st,
            controller::PreferenceChange::Pin(3031)
        )
        .is_err());
    }

    #[test]
    fn champion_and_new_match_boundaries_finish_recap_and_clear_preferences() {
        let (st, handle) = app();
        controller::change_preference(
            &handle,
            &st,
            controller::PreferenceChange::Mode(BuildPreference::Survival),
        )
        .unwrap();
        let _planning = st.planning.lock().unwrap();
        let before = st.session.lock().unwrap().generation;
        assert!(controller::ensure_champion_locked(
            &handle,
            &st,
            Some("Lux")
        ));
        assert_eq!(
            *st.preferences.lock().unwrap(),
            PlannerPreferences::default()
        );
        assert_eq!(st.session.lock().unwrap().generation, before + 1);
        assert_eq!(st.snapshot().recap.unwrap().champion, "Xayah");
        controller::reset_session_locked(&handle, &st);
        assert_eq!(st.session.lock().unwrap().generation, before + 2);
        assert!(st.latest_live.lock().unwrap().is_none());
    }

    #[test]
    fn feedback_updates_the_current_recap_and_duplicate_clicks_are_idempotent() {
        let (st, handle) = app();
        {
            let _planning = st.planning.lock().unwrap();
            controller::finish_journal_locked(&handle, &st);
        }
        let id = st.snapshot().recap.unwrap().decisions[0].id.clone();
        controller::rate_decision(&handle, &st, &id, Feedback::Useful).unwrap();
        controller::rate_decision(&handle, &st, &id, Feedback::Useful).unwrap();
        assert_eq!(
            st.snapshot().recap.unwrap().decisions[0].feedback,
            Some(Feedback::Useful)
        );
        controller::rate_decision(&handle, &st, &id, Feedback::NotUseful).unwrap();
        assert_eq!(
            st.snapshot().recap.unwrap().decisions[0].feedback,
            Some(Feedback::NotUseful)
        );
    }

    #[test]
    fn the_actual_poller_future_remains_send_without_ever_running_it() {
        fn assert_send<T: Send>(_: T) {}
        let (st, handle) = app();
        assert_send(poller::run(handle, st));
    }
}
