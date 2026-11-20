import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, CheckCircle2, Copy, Globe, Info, Loader2 } from 'lucide-react';
import { request } from '../../utils/request';
import { copyToClipboard } from '../../utils/clipboard';
import { useGeminiAppAccounts } from '../../stores/useGeminiAppAccounts';
import { GeminiAppTerms } from './GeminiAppTerms';
import { isTauri } from '../../utils/env';

interface Flow { flow_id: string; authorization_url: string }
interface FlowStatus { status: 'pending' | 'completed' | 'failed' | 'cancelled'; error: string | null }
type GatewayStatus = 'configured' | 'invalid' | null;

export default function GeminiAppTab() {
    const { t } = useTranslation();
    const accounts = useGeminiAppAccounts(state => state.accounts);
    const refresh = useGeminiAppAccounts(state => state.refresh);
    const removeAccount = useGeminiAppAccounts(state => state.remove);
    const [copied, setCopied] = useState(false);
    const [flow, setFlow] = useState<Flow>();
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState('');
    const [success, setSuccess] = useState(false);
    const [confirmId, setConfirmId] = useState<string>();
    const [gatewayStatus, setGatewayStatus] = useState<GatewayStatus>();
    const mounted = useRef(false);
    const active = useRef<string | undefined>(undefined);

    const cancel = (id: string) => request('gemini_app_login_cancel', { id });
    const errorCode = (err: unknown) => typeof err === 'string' ? err : 'request_failed';

    useEffect(() => {
        mounted.current = true;
        let disposed = false;
        void request<GatewayStatus>('gemini_app_gateway_status').then(status => {
            if (disposed) return;
            setGatewayStatus(status);
            if (status === null) return refresh();
        }).catch((err: unknown) => { if (!disposed) setError(errorCode(err)); });
        return () => {
            disposed = true;
            mounted.current = false;
            const id = active.current;
            active.current = undefined;
            if (id) void cancel(id).catch(() => {});
        };
    }, []);

    useEffect(() => {
        if (!flow) return;
        let stop = false;
        let timer: ReturnType<typeof setTimeout>;
        const poll = async () => {
            try {
                const result = await request<FlowStatus>('gemini_app_login_status', { id: flow.flow_id });
                if (stop || active.current !== flow.flow_id) return;
                if (result.status === 'pending') {
                    timer = setTimeout(poll, 1000);
                    return;
                }
                active.current = undefined;
                setFlow(undefined);
                if (result.status === 'completed') { setSuccess(true); await refresh(); }
                else if (result.status === 'failed') setError(result.error ?? 'request_failed');
            } catch (err) {
                if (!stop) {
                    const id = active.current;
                    active.current = undefined;
                    setFlow(undefined);
                    setError(errorCode(err));
                    if (id) void cancel(id).catch(() => {});
                }
            }
        };
        timer = setTimeout(poll, 500);
        return () => { stop = true; clearTimeout(timer); };
    }, [flow]);

    async function openGoogle(url: string) {
        if (isTauri()) {
            const { openUrl } = await import('@tauri-apps/plugin-opener');
            await openUrl(url);
        } else {
            window.open(url, '_blank', 'noopener,noreferrer');
        }
    }

    async function start() {
        setBusy(true);
        setError('');
        setSuccess(false);
        try {
            const next = await request<Flow>('gemini_app_login_start');
            if (!mounted.current) { await cancel(next.flow_id); return; }
            active.current = next.flow_id;
            setCopied(false);
            setFlow(next);
            if (isTauri()) await openGoogle(next.authorization_url);
        } catch (err) { if (mounted.current) setError(errorCode(err)); }
        finally { if (mounted.current) setBusy(false); }
    }

    async function stop() {
        if (!flow) return;
        setBusy(true);
        try {
            await cancel(flow.flow_id);
            active.current = undefined;
            setFlow(undefined);
            await refresh();
        } catch (err) { if (mounted.current) setError(errorCode(err)); }
        finally { if (mounted.current) setBusy(false); }
    }

    async function remove(id: string) {
        setBusy(true);
        setError('');
        setSuccess(false);
        try {
            await removeAccount(id);
            setConfirmId(undefined);
        } catch (err) { if (mounted.current) setError(errorCode(err)); }
        finally { if (mounted.current) setBusy(false); }
    }

    const prefix = 'accounts.add.gemini_app';
    return (
        <section className="space-y-4 py-4">
            <div className="text-center space-y-3">
                <div className="bg-blue-50 dark:bg-blue-900/20 rounded-full w-20 h-20 mx-auto flex items-center justify-center">
                    <Globe className="w-10 h-10 text-blue-500" aria-hidden="true" />
                </div>
                <div className="space-y-1">
                    <h4 className="font-medium text-gray-900 dark:text-gray-100">{gatewayStatus ? 'Gemini App2API' : 'Gemini App OAuth'}</h4>
                    <p className="text-sm text-gray-500 dark:text-gray-400 max-w-xs mx-auto">{t(`${prefix}.${gatewayStatus === 'configured' ? 'gateway_description' : gatewayStatus === 'invalid' ? 'gateway_config_invalid' : 'description'}`)}</p>
                </div>
            </div>
            {gatewayStatus === null && !isTauri() && <p className="flex items-start gap-2 rounded-xl border border-blue-100 dark:border-blue-900/40 bg-blue-50 dark:bg-blue-900/20 p-2.5 text-xs text-blue-700 dark:text-blue-300"><Info className="h-4 w-4 shrink-0" />{t(`${prefix}.web_hint`)}</p>}
            {error && <p role="alert" className="rounded-xl bg-red-50 dark:bg-red-900/20 p-3 text-sm text-red-700 dark:text-red-300">{t(`${prefix}.errors.${error}`, { defaultValue: t(`${prefix}.errors.request_failed`) })}</p>}
            {success && <p role="status" className="flex items-start gap-2 rounded-xl bg-emerald-50 dark:bg-emerald-900/20 p-3 text-sm text-emerald-700 dark:text-emerald-300"><CheckCircle2 className="w-4 h-4 shrink-0" />{t(`${prefix}.success`)}</p>}
            {gatewayStatus === null && <>
            {flow ? (
                <div className="space-y-2">
                    <p role="status" className="flex items-center justify-center gap-2 py-1 text-sm text-blue-700 dark:text-blue-300"><Loader2 className="h-4 w-4 animate-spin" />{t(`${prefix}.waiting`)}</p>
                    <a className="w-full px-4 py-3 bg-blue-600 hover:bg-blue-700 text-white font-medium rounded-xl shadow-lg shadow-blue-500/20 transition-all flex items-center justify-center gap-2" href={flow.authorization_url} target="_blank" rel="noreferrer" onClick={isTauri() ? event => { event.preventDefault(); void openGoogle(flow.authorization_url).catch(err => setError(errorCode(err))); } : undefined}>{t(`${prefix}.open_google`)}</a>
                    <button type="button" className="w-full min-w-0 px-3 py-2 rounded-xl border border-dashed border-gray-300 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-base-200 transition-colors flex items-center gap-2" onClick={async () => { if (await copyToClipboard(flow.authorization_url)) setCopied(true); else setError('copy_failed'); }}>
                        {copied ? <Check className="h-4 w-4 shrink-0 text-emerald-500" /> : <Copy className="h-4 w-4 shrink-0" />}
                        <span className="min-w-0 flex-1 truncate text-left text-xs">{t('accounts.add.oauth.link_label')}</span>
                        <span className="shrink-0 text-xs">{t(copied ? 'accounts.add.oauth.copied' : 'accounts.add.oauth.copy_link')}</span>
                    </button>
                    <button type="button" className="w-full rounded-xl px-4 py-2 text-sm hover:bg-gray-100 dark:hover:bg-base-200 disabled:opacity-60" disabled={busy} onClick={() => void stop()}>{t('common.cancel')}</button>
                </div>
            ) : (
                <button type="button" className="w-full px-4 py-3 bg-blue-600 hover:bg-blue-700 text-white font-medium rounded-xl shadow-lg shadow-blue-500/20 transition-all flex items-center justify-center gap-2 disabled:opacity-70 disabled:cursor-not-allowed" disabled={busy} onClick={() => void start()}>{busy && <Loader2 className="h-4 w-4 animate-spin" />}{t(`${prefix}.start`)}</button>
            )}
            <h4 className="border-t border-gray-100 dark:border-base-200 pt-3 text-sm font-semibold">{t(`${prefix}.pool`)}</h4>
            {accounts.length === 0 && <p className="text-xs opacity-70">{t(`${prefix}.empty`)}</p>}
            <ul className="space-y-2">
                {accounts.map(account => (
                    <li key={account.id} className="rounded-lg bg-base-200 p-2 text-sm">
                        <div className="flex flex-wrap items-start justify-between gap-2">
                            <div className="min-w-0 break-all"><div>{account.email}</div><div className="text-xs opacity-75">{t(`${prefix}.${account.needs_login ? 'needs_login' : 'authorized'}`)}</div></div>
                            <button type="button" className="btn btn-ghost btn-xs" disabled={busy || Boolean(flow)} onClick={() => setConfirmId(account.id)}>{t(`${prefix}.remove`)}</button>
                        </div>
                        <GeminiAppTerms account={account} disabled={busy || Boolean(flow)} />
                        {confirmId === account.id && <div className="mt-2 flex flex-wrap items-center gap-2">
                            <span>{t(`${prefix}.confirm_remove`)}</span>
                            <button type="button" className="btn btn-error btn-xs" disabled={busy} onClick={() => void remove(account.id)}>{t(`${prefix}.remove`)}</button>
                            <button type="button" className="btn btn-xs" disabled={busy} onClick={() => setConfirmId(undefined)}>{t('common.cancel')}</button>
                        </div>}
                    </li>
                ))}
            </ul>
            </>}
        </section>
    );
}
