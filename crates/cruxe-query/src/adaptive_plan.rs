use cruxe_core::config::{AdaptivePlanConfig, SearchConfig};
use cruxe_core::types::QueryIntent;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryPlan {
    LexicalFast,
    HybridStandard,
    SemanticDeep,
}

impl QueryPlan {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LexicalFast => "lexical_fast",
            Self::HybridStandard => "hybrid_standard",
            Self::SemanticDeep => "semantic_deep",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "lexical_fast" | "fast" | "lexical" => Some(Self::LexicalFast),
            "hybrid_standard" | "standard" | "hybrid" => Some(Self::HybridStandard),
            "semantic_deep" | "deep" | "semantic" => Some(Self::SemanticDeep),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionReason {
    Override,
    SemanticUnavailable,
    HighLexicalConfidence,
    LowLexicalConfidenceExploratory,
    DefaultHybrid,
    DisabledFallback,
}

impl SelectionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::SemanticUnavailable => "semantic_unavailable_rule",
            Self::HighLexicalConfidence => "high_confidence_lexical_rule",
            Self::LowLexicalConfidenceExploratory => "low_confidence_exploratory_rule",
            Self::DefaultHybrid => "default_hybrid_rule",
            Self::DisabledFallback => "adaptive_plan_disabled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DowngradeReason {
    SemanticUnavailable,
    BudgetExhausted,
    TimeoutGuard,
    ConfigForced,
}

impl DowngradeReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SemanticUnavailable => "semantic_unavailable",
            Self::BudgetExhausted => "budget_exhausted",
            Self::TimeoutGuard => "timeout_guard",
            Self::ConfigForced => "config_forced",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlanSelectionInput<'a> {
    pub intent: QueryIntent,
    pub lexical_confidence: f64,
    pub semantic_runtime_available: bool,
    pub override_plan: Option<&'a str>,
    pub config: &'a AdaptivePlanConfig,
}

#[derive(Debug, Clone, Copy)]
pub struct PlanBudget {
    pub semantic_limit: usize,
    pub lexical_fanout: usize,
    pub semantic_fanout: usize,
    pub latency_budget_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct PlanController {
    pub selected: QueryPlan,
    pub executed: QueryPlan,
    pub selection_reason: SelectionReason,
    pub downgraded: bool,
    pub downgrade_reason: Option<DowngradeReason>,
}

impl PlanController {
    pub fn select(input: PlanSelectionInput<'_>) -> Self {
        let selected = if !input.config.enabled {
            QueryPlan::HybridStandard
        } else if input.config.allow_override {
            input
                .override_plan
                .and_then(QueryPlan::parse)
                .unwrap_or_else(|| {
                    select_without_override(
                        input.intent,
                        input.lexical_confidence,
                        input.semantic_runtime_available,
                        input.config,
                    )
                })
        } else {
            select_without_override(
                input.intent,
                input.lexical_confidence,
                input.semantic_runtime_available,
                input.config,
            )
        };

        let selection_reason = if !input.config.enabled {
            SelectionReason::DisabledFallback
        } else if input.config.allow_override
            && input.override_plan.is_some()
            && input.override_plan.and_then(QueryPlan::parse).is_some()
        {
            SelectionReason::Override
        } else {
            select_reason_without_override(
                input.intent,
                input.lexical_confidence,
                input.semantic_runtime_available,
                input.config,
            )
        };

        let mut controller = Self {
            selected,
            executed: selected,
            selection_reason,
            downgraded: false,
            downgrade_reason: None,
        };

        // One-way guard: selected deep cannot execute without semantic runtime.
        if controller.executed == QueryPlan::SemanticDeep && !input.semantic_runtime_available {
            controller.downgrade(DowngradeReason::SemanticUnavailable);
        }
        // If plan was forced to deep and runtime is still unavailable after one-step
        // downgrade, allow another one-way downgrade to lexical_fast.
        if controller.executed == QueryPlan::HybridStandard
            && !input.semantic_runtime_available
            && selection_reason == SelectionReason::Override
        {
            controller.downgrade(DowngradeReason::SemanticUnavailable);
        }

        record_selected_plan(controller.selected);
        controller
    }

    pub fn downgrade(&mut self, reason: DowngradeReason) {
        let next = match self.executed {
            QueryPlan::SemanticDeep => QueryPlan::HybridStandard,
            QueryPlan::HybridStandard => QueryPlan::LexicalFast,
            QueryPlan::LexicalFast => QueryPlan::LexicalFast,
        };
        if next == self.executed {
            return;
        }
        self.executed = next;
        self.downgraded = true;
        if self.downgrade_reason.is_none() {
            self.downgrade_reason = Some(reason);
        }
        record_downgrade_reason(reason);
    }

    pub fn ensure_latency_budget(&mut self, elapsed_ms: u64, budget: PlanBudget) {
        if elapsed_ms > budget.latency_budget_ms {
            self.downgrade(DowngradeReason::TimeoutGuard);
        }
    }
}

fn select_without_override(
    intent: QueryIntent,
    lexical_confidence: f64,
    semantic_runtime_available: bool,
    config: &AdaptivePlanConfig,
) -> QueryPlan {
    if !semantic_runtime_available {
        return match intent {
            QueryIntent::Symbol | QueryIntent::Path | QueryIntent::Error => QueryPlan::LexicalFast,
            QueryIntent::NaturalLanguage => QueryPlan::HybridStandard,
        };
    }

    if matches!(
        intent,
        QueryIntent::Symbol | QueryIntent::Path | QueryIntent::Error
    ) && lexical_confidence >= config.high_confidence_threshold
    {
        return QueryPlan::LexicalFast;
    }

    if matches!(intent, QueryIntent::NaturalLanguage)
        && lexical_confidence < config.low_confidence_threshold
        && semantic_runtime_available
    {
        return QueryPlan::SemanticDeep;
    }

    QueryPlan::HybridStandard
}

fn select_reason_without_override(
    intent: QueryIntent,
    lexical_confidence: f64,
    semantic_runtime_available: bool,
    config: &AdaptivePlanConfig,
) -> SelectionReason {
    if !semantic_runtime_available {
        return SelectionReason::SemanticUnavailable;
    }
    if matches!(
        intent,
        QueryIntent::Symbol | QueryIntent::Path | QueryIntent::Error
    ) && lexical_confidence >= config.high_confidence_threshold
    {
        return SelectionReason::HighLexicalConfidence;
    }
    if matches!(intent, QueryIntent::NaturalLanguage)
        && lexical_confidence < config.low_confidence_threshold
    {
        return SelectionReason::LowLexicalConfidenceExploratory;
    }
    SelectionReason::DefaultHybrid
}

pub fn plan_budget(plan: QueryPlan, limit: usize, search_config: &SearchConfig) -> PlanBudget {
    let adaptive = &search_config.adaptive_plan;
    let (semantic_limit_multiplier, lexical_multiplier, semantic_multiplier, latency_budget_ms) =
        match plan {
            QueryPlan::LexicalFast => (
                0,
                adaptive.lexical_fast_lexical_fanout_multiplier,
                0,
                adaptive.lexical_fast_latency_budget_ms,
            ),
            QueryPlan::HybridStandard => (
                adaptive.hybrid_standard_semantic_limit_multiplier,
                adaptive.hybrid_standard_lexical_fanout_multiplier,
                adaptive.hybrid_standard_semantic_fanout_multiplier,
                adaptive.hybrid_standard_latency_budget_ms,
            ),
            QueryPlan::SemanticDeep => (
                adaptive.semantic_deep_semantic_limit_multiplier,
                adaptive.semantic_deep_lexical_fanout_multiplier,
                adaptive.semantic_deep_semantic_fanout_multiplier,
                adaptive.semantic_deep_latency_budget_ms,
            ),
        };

    let semantic_limit = limit
        .saturating_mul(semantic_limit_multiplier)
        .clamp(20, 1000);
    let lexical_fanout = limit.saturating_mul(lexical_multiplier).clamp(40, 2000);
    let semantic_fanout = limit.saturating_mul(semantic_multiplier).clamp(30, 1000);

    PlanBudget {
        semantic_limit: if plan == QueryPlan::LexicalFast {
            0
        } else {
            semantic_limit
        },
        lexical_fanout,
        semantic_fanout: if plan == QueryPlan::LexicalFast {
            0
        } else {
            semantic_fanout
        },
        latency_budget_ms,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdaptivePlanCounters {
    pub selected_lexical_fast: u64,
    pub selected_hybrid_standard: u64,
    pub selected_semantic_deep: u64,
    pub downgrade_semantic_unavailable: u64,
    pub downgrade_budget_exhausted: u64,
    pub downgrade_timeout_guard: u64,
    pub downgrade_config_forced: u64,
}

static SELECTED_LEXICAL_FAST: AtomicU64 = AtomicU64::new(0);
static SELECTED_HYBRID_STANDARD: AtomicU64 = AtomicU64::new(0);
static SELECTED_SEMANTIC_DEEP: AtomicU64 = AtomicU64::new(0);
static DOWNGRADE_SEMANTIC_UNAVAILABLE: AtomicU64 = AtomicU64::new(0);
static DOWNGRADE_BUDGET_EXHAUSTED: AtomicU64 = AtomicU64::new(0);
static DOWNGRADE_TIMEOUT_GUARD: AtomicU64 = AtomicU64::new(0);
static DOWNGRADE_CONFIG_FORCED: AtomicU64 = AtomicU64::new(0);

fn record_selected_plan(plan: QueryPlan) {
    match plan {
        QueryPlan::LexicalFast => {
            SELECTED_LEXICAL_FAST.fetch_add(1, Ordering::Relaxed);
        }
        QueryPlan::HybridStandard => {
            SELECTED_HYBRID_STANDARD.fetch_add(1, Ordering::Relaxed);
        }
        QueryPlan::SemanticDeep => {
            SELECTED_SEMANTIC_DEEP.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn record_downgrade_reason(reason: DowngradeReason) {
    match reason {
        DowngradeReason::SemanticUnavailable => {
            DOWNGRADE_SEMANTIC_UNAVAILABLE.fetch_add(1, Ordering::Relaxed);
        }
        DowngradeReason::BudgetExhausted => {
            DOWNGRADE_BUDGET_EXHAUSTED.fetch_add(1, Ordering::Relaxed);
        }
        DowngradeReason::TimeoutGuard => {
            DOWNGRADE_TIMEOUT_GUARD.fetch_add(1, Ordering::Relaxed);
        }
        DowngradeReason::ConfigForced => {
            DOWNGRADE_CONFIG_FORCED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn snapshot_counters() -> AdaptivePlanCounters {
    AdaptivePlanCounters {
        selected_lexical_fast: SELECTED_LEXICAL_FAST.load(Ordering::Relaxed),
        selected_hybrid_standard: SELECTED_HYBRID_STANDARD.load(Ordering::Relaxed),
        selected_semantic_deep: SELECTED_SEMANTIC_DEEP.load(Ordering::Relaxed),
        downgrade_semantic_unavailable: DOWNGRADE_SEMANTIC_UNAVAILABLE.load(Ordering::Relaxed),
        downgrade_budget_exhausted: DOWNGRADE_BUDGET_EXHAUSTED.load(Ordering::Relaxed),
        downgrade_timeout_guard: DOWNGRADE_TIMEOUT_GUARD.load(Ordering::Relaxed),
        downgrade_config_forced: DOWNGRADE_CONFIG_FORCED.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
pub fn reset_counters_for_test() {
    SELECTED_LEXICAL_FAST.store(0, Ordering::Relaxed);
    SELECTED_HYBRID_STANDARD.store(0, Ordering::Relaxed);
    SELECTED_SEMANTIC_DEEP.store(0, Ordering::Relaxed);
    DOWNGRADE_SEMANTIC_UNAVAILABLE.store(0, Ordering::Relaxed);
    DOWNGRADE_BUDGET_EXHAUSTED.store(0, Ordering::Relaxed);
    DOWNGRADE_TIMEOUT_GUARD.store(0, Ordering::Relaxed);
    DOWNGRADE_CONFIG_FORCED.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AdaptivePlanConfig {
        AdaptivePlanConfig::default()
    }

    #[test]
    fn selector_prefers_override_when_allowed() {
        let selection = PlanController::select(PlanSelectionInput {
            intent: QueryIntent::NaturalLanguage,
            lexical_confidence: 0.1,
            semantic_runtime_available: true,
            override_plan: Some("lexical_fast"),
            config: &cfg(),
        });
        assert_eq!(selection.selected, QueryPlan::LexicalFast);
        assert_eq!(selection.selection_reason, SelectionReason::Override);
    }

    #[test]
    fn selector_falls_back_when_semantic_unavailable() {
        let selection = PlanController::select(PlanSelectionInput {
            intent: QueryIntent::NaturalLanguage,
            lexical_confidence: 0.1,
            semantic_runtime_available: false,
            override_plan: None,
            config: &cfg(),
        });
        assert_eq!(selection.selected, QueryPlan::HybridStandard);
        assert_eq!(
            selection.selection_reason,
            SelectionReason::SemanticUnavailable
        );
    }

    #[test]
    fn selector_applies_high_confidence_symbol_rule() {
        let selection = PlanController::select(PlanSelectionInput {
            intent: QueryIntent::Symbol,
            lexical_confidence: 0.9,
            semantic_runtime_available: true,
            override_plan: None,
            config: &cfg(),
        });
        assert_eq!(selection.selected, QueryPlan::LexicalFast);
        assert_eq!(
            selection.selection_reason,
            SelectionReason::HighLexicalConfidence
        );
    }

    #[test]
    fn selector_applies_low_confidence_nl_rule() {
        let selection = PlanController::select(PlanSelectionInput {
            intent: QueryIntent::NaturalLanguage,
            lexical_confidence: 0.2,
            semantic_runtime_available: true,
            override_plan: None,
            config: &cfg(),
        });
        assert_eq!(selection.selected, QueryPlan::SemanticDeep);
        assert_eq!(
            selection.selection_reason,
            SelectionReason::LowLexicalConfidenceExploratory
        );
    }

    #[test]
    fn selector_uses_default_hybrid_otherwise() {
        let selection = PlanController::select(PlanSelectionInput {
            intent: QueryIntent::NaturalLanguage,
            lexical_confidence: 0.72,
            semantic_runtime_available: true,
            override_plan: None,
            config: &cfg(),
        });
        assert_eq!(selection.selected, QueryPlan::HybridStandard);
        assert_eq!(selection.selection_reason, SelectionReason::DefaultHybrid);
    }

    #[test]
    fn override_deep_downgrades_when_semantic_runtime_unavailable() {
        let selection = PlanController::select(PlanSelectionInput {
            intent: QueryIntent::NaturalLanguage,
            lexical_confidence: 0.2,
            semantic_runtime_available: false,
            override_plan: Some("semantic_deep"),
            config: &cfg(),
        });
        assert!(selection.downgraded);
        assert_eq!(selection.executed, QueryPlan::LexicalFast);
        assert_eq!(
            selection.downgrade_reason,
            Some(DowngradeReason::SemanticUnavailable)
        );
    }

    #[test]
    fn plan_budget_scales_by_plan() {
        let search = SearchConfig::default();
        let lexical = plan_budget(QueryPlan::LexicalFast, 10, &search);
        let hybrid = plan_budget(QueryPlan::HybridStandard, 10, &search);
        let deep = plan_budget(QueryPlan::SemanticDeep, 10, &search);
        assert_eq!(lexical.semantic_limit, 0);
        assert!(hybrid.semantic_limit > 0);
        assert!(deep.semantic_limit >= hybrid.semantic_limit);
        assert!(deep.lexical_fanout >= hybrid.lexical_fanout);
    }

    #[test]
    fn timeout_guard_downgrades_one_way() {
        let mut controller = PlanController {
            selected: QueryPlan::SemanticDeep,
            executed: QueryPlan::SemanticDeep,
            selection_reason: SelectionReason::DefaultHybrid,
            downgraded: false,
            downgrade_reason: None,
        };
        controller.ensure_latency_budget(
            500,
            PlanBudget {
                semantic_limit: 10,
                lexical_fanout: 20,
                semantic_fanout: 20,
                latency_budget_ms: 100,
            },
        );
        assert!(controller.downgraded);
        assert_eq!(controller.executed, QueryPlan::HybridStandard);
        assert_eq!(
            controller.downgrade_reason,
            Some(DowngradeReason::TimeoutGuard)
        );
    }

    #[test]
    fn counters_track_selection_and_downgrade() {
        reset_counters_for_test();
        let mut controller = PlanController::select(PlanSelectionInput {
            intent: QueryIntent::NaturalLanguage,
            lexical_confidence: 0.1,
            semantic_runtime_available: true,
            override_plan: None,
            config: &cfg(),
        });
        controller.downgrade(DowngradeReason::BudgetExhausted);
        let counters = snapshot_counters();
        assert!(
            counters.selected_semantic_deep >= 1,
            "expected semantic_deep selection counter to increase"
        );
        assert!(
            counters.downgrade_budget_exhausted >= 1,
            "expected budget_exhausted downgrade counter to increase"
        );
    }
}
