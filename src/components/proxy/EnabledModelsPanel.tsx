import { Check, Minus, RefreshCw } from 'lucide-react';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { create } from 'zustand';
import { cn } from '../../utils/cn';
import { request } from '../../utils/request';

const MODEL_CATALOG_CACHE_KEY = 'antigravity_proxy_model_catalog_groups_v2';

interface ModelGroup {
    id: string;
    members: string[];
}

interface ModelCatalogState {
    catalog: ModelGroup[] | null;
    loading: boolean;
    error: string | null;
    refresh: () => Promise<void>;
}

function isModelCatalog(value: unknown): value is ModelGroup[] {
    return Array.isArray(value) && value.every((group) =>
        group !== null && typeof group === 'object' && typeof group.id === 'string'
        && Array.isArray(group.members) && group.members.every((member: unknown) => typeof member === 'string')
        && group.members.includes(group.id),
    );
}

function readCachedCatalog(): ModelGroup[] | null {
    try {
        const saved = localStorage.getItem(MODEL_CATALOG_CACHE_KEY);
        const catalog: unknown = saved === null ? null : JSON.parse(saved);
        return isModelCatalog(catalog) ? catalog : null;
    } catch {
        return null;
    }
}

const useModelCatalog = create<ModelCatalogState>((set, get) => ({
    catalog: readCachedCatalog(),
    loading: false,
    error: null,
    refresh: async () => {
        if (get().loading) return;
        set({ loading: true, error: null });
        try {
            const catalog = await request<unknown>('get_proxy_model_catalog');
            if (!isModelCatalog(catalog)) throw new Error('Invalid model catalog');
            try {
                localStorage.setItem(MODEL_CATALOG_CACHE_KEY, JSON.stringify(catalog));
            } catch {
                // Keep the refreshed catalog available for this session.
            }
            set({ catalog });
        } catch (error) {
            set({ error: String(error) });
        } finally {
            set({ loading: false });
        }
    },
}));

interface EnabledModelsPanelProps {
    value: string[] | null;
    dirty: boolean;
    disabled: boolean;
    onChange: (value: string[] | null) => void;
}

export default function EnabledModelsPanel({ value, dirty, disabled, onChange }: EnabledModelsPanelProps) {
    const { t } = useTranslation();
    const { catalog, loading, error, refresh } = useModelCatalog();
    useEffect(() => {
        if (catalog === null && !error) void refresh();
    }, [catalog, error, refresh]);

    const groups = catalog ?? [];
    const normalize = (id: string) => id.trim().replace(/^models\//, '').toLowerCase();
    const enabled = new Set(value?.map(normalize));
    const isEnabled = (id: string) => value === null || enabled.has(normalize(id));
    const visibleEnabledCount = groups.filter(({ members }) => members.some(isEnabled)).length;
    const controlsDisabled = disabled || loading;

    const toggleModel = (group: ModelGroup) => {
        const selected = value ?? groups.flatMap(({ members }) => members);
        const groupIds = new Set(group.members.map(normalize));
        const others = selected.filter((id) => !groupIds.has(normalize(id)));
        onChange(group.members.every(isEnabled) ? others : [...others, ...group.members].sort());
    };

    return (
        <section aria-labelledby="enabled-models-title" className="border-t border-gray-100 dark:border-base-200 pt-3.5 space-y-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
                <h3 id="enabled-models-title" className="text-xs font-semibold text-gray-700 dark:text-gray-200">
                    {t('proxy.config.enabled_models.title')}
                </h3>
                <div className="flex flex-wrap items-center gap-2">
                    {value !== null && (
                        <button type="button" disabled={controlsDisabled} onClick={() => onChange(null)} className="btn btn-ghost btn-xs">
                            {t('proxy.config.enabled_models.enable_all')}
                        </button>
                    )}
                    {(value === null ? groups.length > 0 : value.length > 0) && (
                        <button type="button" disabled={controlsDisabled} onClick={() => onChange([])} className="btn btn-ghost btn-xs">
                            {t('proxy.config.enabled_models.disable_all')}
                        </button>
                    )}
                    <button type="button" disabled={controlsDisabled} onClick={refresh} className="btn btn-outline btn-xs gap-1.5">
                        <RefreshCw size={12} aria-hidden="true" className={cn(loading && 'animate-spin')} />
                        {t('proxy.config.enabled_models.fetch')}
                    </button>
                </div>
            </div>
            <p className="text-xs text-gray-500 dark:text-gray-400" role="status">
                {loading && catalog === null
                    ? t('proxy.config.enabled_models.loading')
                    : dirty
                        ? t('proxy.config.enabled_models.unsaved')
                        : value === null
                            ? t('proxy.config.enabled_models.all_enabled')
                            : catalog !== null && t('proxy.config.enabled_models.selected', { count: visibleEnabledCount })}
            </p>
            {groups.some(({ members }) => members.length > 1) && (
                <p className="text-xs text-gray-500 dark:text-gray-400">
                    {t('proxy.config.enabled_models.grouping_hint')}
                </p>
            )}
            {error && (
                <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
                    {t('proxy.config.enabled_models.fetch_failed', { error })}
                </p>
            )}
            {catalog?.length === 0 && (
                <p className="text-xs text-gray-500 dark:text-gray-400">{t('proxy.config.enabled_models.empty')}</p>
            )}
            {groups.length > 0 && (
                <div role="group" aria-labelledby="enabled-models-title" aria-busy={loading} className="flex flex-wrap gap-1.5">
                    {groups.map((group) => {
                        const active = group.members.every(isEnabled);
                        const partial = !active && group.members.some(isEnabled);
                        return (
                            <button
                                key={group.id}
                                type="button"
                                role="checkbox"
                                aria-checked={partial ? 'mixed' : active}
                                disabled={controlsDisabled}
                                onClick={() => toggleModel(group)}
                                className={cn(
                                    'inline-flex items-center gap-1 max-w-full rounded-full border px-2.5 py-1 text-xs font-mono break-all transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-500 disabled:opacity-60',
                                    (active || partial)
                                        ? 'border-blue-200 bg-blue-50 text-blue-700 dark:border-blue-800 dark:bg-blue-950/40 dark:text-blue-300'
                                        : 'border-gray-200 bg-gray-50 text-gray-500 hover:border-gray-400 dark:border-base-300 dark:bg-base-200 dark:text-gray-400',
                                )}
                            >
                                {active && <Check size={12} aria-hidden="true" className="shrink-0" />}
                                {partial && <Minus size={12} aria-hidden="true" className="shrink-0" />}
                                {group.id}
                            </button>
                        );
                    })}
                </div>
            )}
        </section>
    );
}
