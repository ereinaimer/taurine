-- ============================================================================
-- Taurine Cloud Baseline Schema
-- Execute in Supabase SQL Editor or apply via Supabase CLI.
-- ============================================================================

-- ----------------------------------------------------------------------------
-- 1. Snippets (Trigger Configurations and Multi-Aliases)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.snippets (
    id           TEXT        PRIMARY KEY,
    user_id      UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    workspace_id TEXT        NOT NULL DEFAULT 'default',
    name         TEXT        NOT NULL DEFAULT '',
    output       TEXT        NOT NULL DEFAULT '',
    description  TEXT,
    action_type  TEXT        NOT NULL DEFAULT 'text',
    target_os    TEXT,
    only_apps    TEXT,
    except_apps  TEXT,
    tags         TEXT,
    is_enabled   BOOLEAN     NOT NULL DEFAULT true,
    auto_case    BOOLEAN     NOT NULL DEFAULT false,
    version      BIGINT      NOT NULL DEFAULT 1,
    is_deleted   BOOLEAN     NOT NULL DEFAULT false,
    aliases      JSONB       NOT NULL DEFAULT '[]'::jsonb,
    created_at   BIGINT      NOT NULL DEFAULT 0,
    updated_at   BIGINT      NOT NULL DEFAULT 0
);

ALTER TABLE public.snippets ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users can manage their own snippets" ON public.snippets;
CREATE POLICY "Users can manage their own snippets"
    ON public.snippets
    FOR ALL
    USING (auth.uid() = user_id)
    WITH CHECK (auth.uid() = user_id);

CREATE INDEX IF NOT EXISTS idx_snippets_user_updated
    ON public.snippets (user_id, updated_at DESC);

GRANT ALL ON TABLE public.snippets TO authenticated;
GRANT ALL ON TABLE public.snippets TO service_role;

-- ----------------------------------------------------------------------------
-- 2. Quota Ledger (Weekly Usage and Cryptographic Validation)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.quota_ledger (
    user_id              UUID             NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    week_start_epoch     BIGINT           NOT NULL,
    remaining_percentage DOUBLE PRECISION NOT NULL DEFAULT 100.0,
    last_expansion_at    BIGINT           NOT NULL DEFAULT 0,
    updated_at           BIGINT           NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, week_start_epoch)
);

ALTER TABLE public.quota_ledger ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users can manage their own quota ledger" ON public.quota_ledger;
CREATE POLICY "Users can manage their own quota ledger"
    ON public.quota_ledger
    FOR ALL
    USING (auth.uid() = user_id)
    WITH CHECK (auth.uid() = user_id);

GRANT ALL ON TABLE public.quota_ledger TO authenticated;
GRANT ALL ON TABLE public.quota_ledger TO service_role;

-- ----------------------------------------------------------------------------
-- 3. User Settings Store (Portable Key-Value Config Sync)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.user_settings (
    user_id    UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    key        TEXT        NOT NULL,
    value      JSONB       NOT NULL,
    version    BIGINT      NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, key)
);

ALTER TABLE public.user_settings ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users can manage their own settings" ON public.user_settings;
CREATE POLICY "Users can manage their own settings"
    ON public.user_settings
    FOR ALL
    USING (auth.uid() = user_id)
    WITH CHECK (auth.uid() = user_id);

GRANT ALL ON TABLE public.user_settings TO authenticated;
GRANT ALL ON TABLE public.user_settings TO service_role;

-- ----------------------------------------------------------------------------
-- 4. Per-Trigger Daily Dimension Stats
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.trigger_daily_stats (
    user_id        UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    trigger_id     TEXT        NOT NULL,
    date           DATE        NOT NULL,
    app_key        TEXT        NOT NULL DEFAULT '',
    executions     BIGINT      NOT NULL DEFAULT 0,
    words_spoken   BIGINT      NOT NULL DEFAULT 0,
    words_expanded BIGINT      NOT NULL DEFAULT 0,
    time_saved_ms  BIGINT      NOT NULL DEFAULT 0,
    version        BIGINT      NOT NULL DEFAULT 1,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, trigger_id, date, app_key)
);

ALTER TABLE public.trigger_daily_stats ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users can manage their own trigger daily stats" ON public.trigger_daily_stats;
CREATE POLICY "Users can manage their own trigger daily stats"
    ON public.trigger_daily_stats
    FOR ALL
    USING (auth.uid() = user_id)
    WITH CHECK (auth.uid() = user_id);

CREATE INDEX IF NOT EXISTS idx_trigger_daily_stats_user_date
    ON public.trigger_daily_stats (user_id, date DESC);

CREATE INDEX IF NOT EXISTS idx_trigger_daily_stats_user_trigger
    ON public.trigger_daily_stats (user_id, trigger_id, date DESC);

GRANT ALL ON TABLE public.trigger_daily_stats TO authenticated;
GRANT ALL ON TABLE public.trigger_daily_stats TO service_role;

-- ----------------------------------------------------------------------------
-- 5. Global Daily Usage Tallies (Daily Account Summaries)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.user_stats (
    user_id          UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    date             DATE        NOT NULL,
    executions       BIGINT      NOT NULL DEFAULT 0,
    ai_executions    BIGINT      NOT NULL DEFAULT 0,
    voice_executions BIGINT      NOT NULL DEFAULT 0,
    words_spoken     BIGINT      NOT NULL DEFAULT 0,
    words_expanded   BIGINT      NOT NULL DEFAULT 0,
    time_saved_ms    BIGINT      NOT NULL DEFAULT 0,
    version          BIGINT      NOT NULL DEFAULT 1,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, date)
);

ALTER TABLE public.user_stats ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users can manage their own daily stats" ON public.user_stats;
CREATE POLICY "Users can manage their own daily stats"
    ON public.user_stats
    FOR ALL
    USING (auth.uid() = user_id)
    WITH CHECK (auth.uid() = user_id);

GRANT ALL ON TABLE public.user_stats TO authenticated;
GRANT ALL ON TABLE public.user_stats TO service_role;

-- ----------------------------------------------------------------------------
-- 6. Application-Level Daily Usage Tallies (Per-App Summaries)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.user_app_stats (
    user_id        UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    app_key        TEXT        NOT NULL,
    date           DATE        NOT NULL,
    executions     BIGINT      NOT NULL DEFAULT 0,
    words_spoken   BIGINT      NOT NULL DEFAULT 0,
    words_expanded BIGINT      NOT NULL DEFAULT 0,
    time_saved_ms  BIGINT      NOT NULL DEFAULT 0,
    version        BIGINT      NOT NULL DEFAULT 1,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, app_key, date)
);

ALTER TABLE public.user_app_stats ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users can manage their own app stats" ON public.user_app_stats;
CREATE POLICY "Users can manage their own app stats"
    ON public.user_app_stats
    FOR ALL
    USING (auth.uid() = user_id)
    WITH CHECK (auth.uid() = user_id);

CREATE INDEX IF NOT EXISTS idx_user_app_stats_lookup
    ON public.user_app_stats (user_id, date DESC);

GRANT ALL ON TABLE public.user_app_stats TO authenticated;
GRANT ALL ON TABLE public.user_app_stats TO service_role;

-- ----------------------------------------------------------------------------
-- 7. Activity Events (Fine-grained Action Telemetry with Output Content)
-- ----------------------------------------------------------------------------
CREATE TABLE public.activity_events (
    id            TEXT        PRIMARY KEY,
    user_id       UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    trigger_id    TEXT,
    timestamp     BIGINT      NOT NULL,
    date          DATE        NOT NULL,
    category      TEXT        NOT NULL,
    app_key       TEXT        NOT NULL DEFAULT '',
    word_count    BIGINT      NOT NULL DEFAULT 0,
    time_saved_ms BIGINT      NOT NULL DEFAULT 0,
    content       TEXT,
    version       BIGINT      NOT NULL DEFAULT 1,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE public.activity_events ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users can manage their own activity events" ON public.activity_events;
CREATE POLICY "Users can manage their own activity events"
    ON public.activity_events
    FOR ALL
    USING (auth.uid() = user_id)
    WITH CHECK (auth.uid() = user_id);

CREATE INDEX IF NOT EXISTS idx_activity_events_user_timestamp
    ON public.activity_events (user_id, timestamp DESC);

GRANT ALL ON TABLE public.activity_events TO authenticated;
GRANT ALL ON TABLE public.activity_events TO service_role;
