-- ============================================================================
-- Taurine Cloud Migration: Settings & Statistics Synchronization
-- Run this script in the Supabase SQL Editor.
-- ============================================================================

-- ----------------------------------------------------------------------------
-- 1. User Settings Store (Key-Value Config Sync)
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
-- 2. Per-Trigger Daily Dimension Stats (Option B Analytics)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.trigger_daily_stats (
    user_id          UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    trigger_id       TEXT        NOT NULL,
    date             DATE        NOT NULL,
    app_key          TEXT        NOT NULL DEFAULT '',
    executions       BIGINT      NOT NULL DEFAULT 0,
    keystrokes_saved BIGINT      NOT NULL DEFAULT 0,
    time_saved_ms    BIGINT      NOT NULL DEFAULT 0,
    version          BIGINT      NOT NULL DEFAULT 1,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
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
-- 3. Global Daily Usage Tallies (Daily Account Summaries)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.user_stats (
    user_id          UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    date             DATE        NOT NULL,
    executions       BIGINT      NOT NULL DEFAULT 0,
    ai_executions    BIGINT      NOT NULL DEFAULT 0,
    voice_executions BIGINT      NOT NULL DEFAULT 0,
    words_dictated   BIGINT      NOT NULL DEFAULT 0,
    keystrokes_saved BIGINT      NOT NULL DEFAULT 0,
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
-- 4. Application-Level Daily Usage Tallies (Per-App Summaries)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.user_app_stats (
    user_id          UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    app_key          TEXT        NOT NULL,
    date             DATE        NOT NULL,
    executions       BIGINT      NOT NULL DEFAULT 0,
    keystrokes_saved BIGINT      NOT NULL DEFAULT 0,
    time_saved_ms    BIGINT      NOT NULL DEFAULT 0,
    version          BIGINT      NOT NULL DEFAULT 1,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
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
