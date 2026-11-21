import { useEffect, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Bot, Eye, Loader2, RefreshCw, Terminal, X } from 'lucide-react';
import { Claude, DeepSeek, Gemini, Grok, HermesAgent, OpenAI, OpenClaw, OpenCode } from '@lobehub/icons';
import deepWriteIcon from '../../assets/deepwrite-icon.png';
import { cn } from '../../utils/cn';
import ModalDialog from '../common/ModalDialog';
import { showToast } from '../common/ToastContainer';
import { agentApps, configureAgent, disconnectAgent, getAgentStatus, getConfigContent, type AgentApp, type AgentStatus } from './agentIntegration';

interface CliSyncCardProps {
    proxyUrl: string;
    apiKey: string;
    className?: string;
}

const icons: Record<AgentApp, ReactNode> = {
    Claude: <Claude size={22} className="text-white" />,
    DeepSeekHarness: <DeepSeek size={24} />,
    Codex: <OpenAI size={22} className="text-white" />,
    OpenCode: <OpenCode size={20} className="text-white" />,
    GrokBuild: <Grok size={22} className="text-white" />,
    Hermes: <HermesAgent.Avatar size={40} shape="square" />,
    OpenClaw: <OpenClaw.Color size={26} />,
    Gemini: <Gemini.Color size={24} />,
    Droid: <Bot size={22} className="text-white" />,
    DeepWrite: <img src={deepWriteIcon} alt="" className="h-full w-full object-contain" />,
};

const iconBackground: Partial<Record<AgentApp, string>> = {
    DeepWrite: 'bg-transparent',
    Claude: 'bg-[#D97757]',
    DeepSeekHarness: 'bg-white text-black',
    Droid: 'bg-orange-500',
};

type StatusResult = { status: AgentStatus; error?: never } | { status?: never; error: string };

export function CliSyncCard({ proxyUrl, apiKey, className }: CliSyncCardProps) {
    const { t } = useTranslation();
    const [statuses, setStatuses] = useState<Partial<Record<AgentApp, StatusResult>>>({});
    const [pending, setPending] = useState<Partial<Record<AgentApp, boolean>>>({});
    const [disconnectApp, setDisconnectApp] = useState<AgentApp | null>(null);
    const [viewing, setViewing] = useState<{ app: AgentApp; file: string; content: string } | null>(null);

    useEffect(() => {
        let cancelled = false;
        for (const app of agentApps) {
            getAgentStatus(app, proxyUrl).then(
                status => { if (!cancelled) setStatuses(previous => ({ ...previous, [app]: { status } })); },
                error => { if (!cancelled) setStatuses(previous => ({ ...previous, [app]: { error: String(error) } })); },
            );
        }
        return () => { cancelled = true; };
    }, [proxyUrl]);

    const names: Record<AgentApp, string> = {
        Claude: 'Claude Code',
        DeepSeekHarness: t('proxy.cli_sync.apps.deepseek_harness', { defaultValue: 'DeepSeek Harness' }),
        Codex: 'Codex AI',
        OpenCode: 'OpenCode',
        GrokBuild: 'Grok Build',
        Hermes: 'Hermes Agent',
        OpenClaw: 'OpenClaw',
        Gemini: t('proxy.cli_sync.apps.gemini'),
        Droid: 'Droid',
        DeepWrite: 'DeepWrite',
    };

    const refresh = async (app: AgentApp) => {
        try {
            const status = await getAgentStatus(app, proxyUrl);
            setStatuses(previous => ({ ...previous, [app]: { status } }));
        } catch (error) {
            setStatuses(previous => ({ ...previous, [app]: { error: String(error) } }));
        }
    };

    const update = async (app: AgentApp, enabled: boolean) => {
        setPending(previous => ({ ...previous, [app]: true }));
        try {
            if (enabled) {
                await configureAgent(app, proxyUrl, apiKey, statuses[app]?.status);
            } else {
                await disconnectAgent(app);
            }
        } catch (error) {
            showToast(t('proxy.cli_sync.integration.error', { error: String(error) }), 'error');
        } finally {
            await refresh(app);
            setPending(previous => ({ ...previous, [app]: false }));
        }
    };

    const viewConfig = async (app: AgentApp, file?: string) => {
        try {
            const content = await getConfigContent(app, file);
            setViewing({ app, file: file ?? '', content });
        } catch (error) {
            showToast(String(error), 'error');
        }
    };

    return (
        <section className={cn('space-y-4', className)}>
            <header className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-1">
                <h2 className="flex items-center gap-2 text-sm font-semibold">
                    <Terminal size={16} className="text-gray-500 dark:text-gray-400" />
                    {t('proxy.cli_sync.integration.title')}
                </h2>
                <p className="text-xs text-gray-500 dark:text-gray-400">
                    {t('proxy.cli_sync.integration.description')}
                </p>
            </header>

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
                {agentApps.map(app => {
                    const result = statuses[app];
                    const status = result?.status;
                    const enabled = Boolean(status?.is_synced || status?.integration_enabled);
                    const busy = Boolean(pending[app]) || !result;
                    const canEnable = Boolean(status?.installed && apiKey.trim());
                    const name = names[app];
                    const stateLabel = !result ? t('proxy.cli_sync.status.detecting')
                        : result.error ? t('proxy.cli_sync.integration.status_error')
                        : status?.is_synced ? t('proxy.cli_sync.integration.connected')
                        : enabled ? t('proxy.cli_sync.integration.connection_changed')
                        : status?.installed ? t('proxy.cli_sync.integration.disconnected')
                        : t('proxy.cli_sync.status.not_installed');
                    const model = status?.current_model || status?.default_model;
                    return (
                        <article key={app} className={cn(
                            'rounded-xl border bg-white/70 p-4 dark:bg-base-100/70',
                            enabled ? 'border-primary/30' : 'border-gray-200/70 dark:border-base-300',
                        )} title={app === 'DeepWrite' && status?.installed ? t('proxy.cli_sync.integration.deepwrite_hint') : undefined}>
                            <div className="flex items-center gap-3">
                                <div className={cn(
                                    'flex h-10 w-10 shrink-0 items-center justify-center overflow-hidden rounded-xl',
                                    iconBackground[app] ?? 'bg-[#18181b]',
                                )}>
                                    {icons[app]}
                                </div>
                                <div className="min-w-0 flex-1">
                                    <h3 className="truncate text-sm font-semibold" title={name}>{name}</h3>
                                    <p className={cn('mt-0.5 text-xs', enabled ? 'text-primary' : 'text-gray-500 dark:text-gray-400')} role="status">
                                        {stateLabel}
                                        {status?.version && status.version !== 'unknown' && (
                                            <span className="text-gray-500 dark:text-gray-400"> · {t('proxy.cli_sync.status.installed', { version: status.version })}</span>
                                        )}
                                    </p>
                                </div>
                                {result?.error ? (
                                    <button type="button" className="btn btn-ghost btn-xs btn-square" title={result.error}
                                        aria-label={t('common.retry')} onClick={() => {
                                            setStatuses(previous => ({ ...previous, [app]: undefined }));
                                            void refresh(app);
                                        }}>
                                        <RefreshCw size={14} />
                                    </button>
                                ) : (
                                    <div className="flex shrink-0 items-center gap-2">
                                        {busy && <Loader2 size={14} className="animate-spin text-gray-400" aria-hidden="true" />}
                                        <input type="checkbox" role="switch" className="toggle toggle-primary toggle-sm"
                                            aria-label={t('proxy.cli_sync.integration.switch', { name })}
                                            aria-busy={busy}
                                            checked={enabled}
                                            disabled={busy || (!enabled && !canEnable)}
                                            onChange={event => {
                                                if (event.target.checked) void update(app, true);
                                                else if (status?.restore_on_disconnect) setDisconnectApp(app);
                                                else void update(app, false);
                                            }}
                                        />
                                    </div>
                                )}
                            </div>
                            {status?.current_base_url && (
                                <div className="mt-3 flex items-center gap-2 border-t border-gray-200/70 pt-2 dark:border-base-300">
                                    <div className="min-w-0 flex-1 text-xs">
                                        {model && <p className="truncate font-medium" title={model}>{model}</p>}
                                        <p className="truncate font-mono text-gray-500 dark:text-gray-400" title={status.current_base_url}>
                                            {status.current_base_url}
                                        </p>
                                    </div>
                                    {app !== 'DeepWrite' && <button type="button" className="btn btn-ghost btn-xs btn-square"
                                        title={t('proxy.cli_sync.btn_view')} aria-label={t('proxy.cli_sync.btn_view')}
                                        onClick={() => void viewConfig(app)}>
                                        <Eye size={14} />
                                    </button>}
                                </div>
                            )}
                        </article>
                    );
                })}
            </div>

            <ModalDialog
                isOpen={disconnectApp !== null}
                title={t('proxy.cli_sync.integration.disconnect_title')}
                message={t('proxy.cli_sync.integration.restore_message', { name: disconnectApp && names[disconnectApp] })}
                onConfirm={() => {
                    if (disconnectApp) void update(disconnectApp, false);
                    setDisconnectApp(null);
                }}
                onCancel={() => setDisconnectApp(null)}
                isDestructive
            />

            {viewing && (
                <div className="fixed inset-0 z-[300] flex items-center justify-center bg-black/50 p-4 backdrop-blur-xs">
                    <section role="dialog" aria-modal="true" aria-labelledby="agent-config-title"
                        className="w-full max-w-2xl space-y-3 rounded-2xl border border-base-300 bg-base-100 p-5 shadow-2xl">
                        <header className="flex items-center justify-between gap-3">
                            <h3 id="agent-config-title" className="font-semibold">
                                {t('proxy.cli_sync.modal.view_title', { name: names[viewing.app] })}
                            </h3>
                            <button type="button" className="btn btn-ghost btn-sm btn-square"
                                aria-label={t('common.close')} onClick={() => setViewing(null)}>
                                <X size={17} />
                            </button>
                        </header>
                        <div className="flex flex-wrap gap-1">
                            {statuses[viewing.app]?.status?.files.map(file => (
                                <button key={file} type="button"
                                    className={cn('btn btn-xs', viewing.file === file && 'btn-primary')}
                                    onClick={() => void viewConfig(viewing.app, file)}>
                                    {file}
                                </button>
                            ))}
                        </div>
                        <pre className="max-h-[50vh] overflow-auto rounded-lg bg-base-200 p-3 text-xs">{viewing.content}</pre>
                    </section>
                </div>
            )}
        </section>
    );
}
