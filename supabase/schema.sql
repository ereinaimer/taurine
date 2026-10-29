-- ============================================================================
-- Taurine Baseline Schema
-- Execute in Supabase SQL Editor or apply via Supabase CLI.
-- ============================================================================

-- ----------------------------------------------------------------------------
-- 1. Snippets (Trigger Configurations and Multi-Aliases)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.snippets (
    id           UUID        PRIMARY KEY,
    user_id      UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    workspace_id UUID        NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000'::uuid,
    output       TEXT        NOT NULL DEFAULT '',
    description  TEXT,
    action_type  TEXT        NOT NULL DEFAULT 'text',
    target_os    TEXT,
    only_apps    TEXT,
    except_apps  TEXT,
    tags         JSONB       NOT NULL DEFAULT '[]'::jsonb,
    is_enabled   BOOLEAN     NOT NULL DEFAULT true,
    auto_case    BOOLEAN     NOT NULL DEFAULT false,
    version      BIGINT      NOT NULL DEFAULT 1,
    is_deleted   BOOLEAN     NOT NULL DEFAULT false,
    aliases      JSONB       NOT NULL DEFAULT '[]'::jsonb,
    created_at   BIGINT      NOT NULL DEFAULT 0,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE public.snippets DROP COLUMN IF EXISTS local_id;
ALTER TABLE public.snippets DROP COLUMN IF EXISTS name;

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
-- 2b. Quota Burn Events (Idempotent Delta Tracking)
-- ----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.quota_burn_events (
    id          UUID             PRIMARY KEY,
    user_id     UUID             NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    device_id   TEXT             NOT NULL DEFAULT '',
    week_epoch  BIGINT           NOT NULL,
    delta       DOUBLE PRECISION NOT NULL,
    created_at  TIMESTAMPTZ      NOT NULL DEFAULT now()
);

ALTER TABLE public.quota_burn_events ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users can manage their own quota burn events" ON public.quota_burn_events;
CREATE POLICY "Users can manage their own quota burn events"
    ON public.quota_burn_events
    FOR ALL
    USING (auth.uid() = user_id)
    WITH CHECK (auth.uid() = user_id);

GRANT ALL ON TABLE public.quota_burn_events TO authenticated;
GRANT ALL ON TABLE public.quota_burn_events TO service_role;

-- ----------------------------------------------------------------------------
-- 2c. Quota Burn RPC (Atomic Delta Deduction with Idempotency)
-- ----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION public.record_quota_burn(
    p_week_epoch BIGINT,
    p_delta DOUBLE PRECISION,
    p_device_id TEXT,
    p_mutation_id TEXT
)
RETURNS JSONB
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE
    v_user_id UUID;
    v_new_balance DOUBLE PRECISION;
    v_mut_uuid UUID;
BEGIN
    v_user_id := auth.uid();
    IF v_user_id IS NULL THEN
        RAISE EXCEPTION 'Not authenticated';
    END IF;

    -- Safely parse or default the mutation UUID
    BEGIN
        v_mut_uuid := p_mutation_id::UUID;
    EXCEPTION WHEN others THEN
        v_mut_uuid := gen_random_uuid();
    END;

    -- Check for idempotency: if already recorded, return current balance
    IF EXISTS (
        SELECT 1 FROM public.quota_burn_events
        WHERE id = v_mut_uuid AND user_id = v_user_id
    ) THEN
        SELECT remaining_percentage INTO v_new_balance
        FROM public.quota_ledger
        WHERE user_id = v_user_id AND week_start_epoch = p_week_epoch;

        RETURN jsonb_build_object('new_balance', COALESCE(v_new_balance, 100.0));
    END IF;

    -- Record the burn event
    INSERT INTO public.quota_burn_events (id, user_id, device_id, week_epoch, delta, created_at)
    VALUES (v_mut_uuid, v_user_id, p_device_id, p_week_epoch, p_delta, now());

    -- Atomically update the quota ledger
    INSERT INTO public.quota_ledger (
        user_id,
        week_start_epoch,
        remaining_percentage,
        last_expansion_at,
        updated_at
    )
    VALUES (
        v_user_id,
        p_week_epoch,
        GREATEST(0.0, 100.0 - p_delta),
        extract(epoch from now())::BIGINT,
        extract(epoch from now())::BIGINT
    )
    ON CONFLICT (user_id, week_start_epoch)
    DO UPDATE SET
        remaining_percentage = GREATEST(0.0, public.quota_ledger.remaining_percentage - p_delta),
        last_expansion_at = extract(epoch from now())::BIGINT,
        updated_at = extract(epoch from now())::BIGINT
    RETURNING remaining_percentage INTO v_new_balance;

    RETURN jsonb_build_object('new_balance', v_new_balance);
END;
$$;

GRANT EXECUTE ON FUNCTION public.record_quota_burn(BIGINT, DOUBLE PRECISION, TEXT, TEXT) TO authenticated;
GRANT EXECUTE ON FUNCTION public.record_quota_burn(BIGINT, DOUBLE PRECISION, TEXT, TEXT) TO service_role;

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
    device_id      TEXT        NOT NULL DEFAULT '',
    trigger_id     TEXT        NOT NULL,
    date           DATE        NOT NULL,
    app_key        TEXT        NOT NULL DEFAULT '',
    executions     BIGINT      NOT NULL DEFAULT 0,
    words_spoken   BIGINT      NOT NULL DEFAULT 0,
    words_expanded BIGINT      NOT NULL DEFAULT 0,
    time_saved_ms  BIGINT      NOT NULL DEFAULT 0,
    version        BIGINT      NOT NULL DEFAULT 1,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, device_id, trigger_id, date, app_key)
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
    device_id        TEXT        NOT NULL DEFAULT '',
    date             DATE        NOT NULL,
    executions       BIGINT      NOT NULL DEFAULT 0,
    ai_executions    BIGINT      NOT NULL DEFAULT 0,
    voice_executions BIGINT      NOT NULL DEFAULT 0,
    words_spoken     BIGINT      NOT NULL DEFAULT 0,
    words_expanded   BIGINT      NOT NULL DEFAULT 0,
    time_saved_ms    BIGINT      NOT NULL DEFAULT 0,
    version          BIGINT      NOT NULL DEFAULT 1,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, device_id, date)
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
    device_id      TEXT        NOT NULL DEFAULT '',
    app_key        TEXT        NOT NULL,
    date           DATE        NOT NULL,
    executions     BIGINT      NOT NULL DEFAULT 0,
    words_spoken   BIGINT      NOT NULL DEFAULT 0,
    words_expanded BIGINT      NOT NULL DEFAULT 0,
    time_saved_ms  BIGINT      NOT NULL DEFAULT 0,
    version        BIGINT      NOT NULL DEFAULT 1,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, device_id, app_key, date)
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
CREATE TABLE IF NOT EXISTS public.activity_events (
    id            TEXT        PRIMARY KEY,
    user_id       UUID        NOT NULL DEFAULT auth.uid() REFERENCES auth.users(id) ON DELETE CASCADE,
    device_id     TEXT        NOT NULL DEFAULT '',
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

-- ----------------------------------------------------------------------------
-- 8. Concurrency Guards: Enforce Monotonic Last-Write-Wins on Cloud Updates
-- ----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION public.reconcile_snippet_upsert()
RETURNS trigger AS $$
BEGIN
    IF NEW.version < OLD.version OR (NEW.version = OLD.version AND NEW.updated_at < OLD.updated_at) THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_reconcile_snippet_upsert ON public.snippets;
CREATE TRIGGER trg_reconcile_snippet_upsert
    BEFORE UPDATE ON public.snippets
    FOR EACH ROW
    EXECUTE FUNCTION public.reconcile_snippet_upsert();

CREATE OR REPLACE FUNCTION public.reconcile_setting_upsert()
RETURNS trigger AS $$
BEGIN
    IF NEW.version < OLD.version THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_reconcile_setting_upsert ON public.user_settings;
CREATE TRIGGER trg_reconcile_setting_upsert
    BEFORE UPDATE ON public.user_settings
    FOR EACH ROW
    EXECUTE FUNCTION public.reconcile_setting_upsert();
