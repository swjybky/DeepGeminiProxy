import { useState, type ReactNode } from 'react';
import { motion } from 'framer-motion';
import { AlertTriangle, FileCheck2, Info, KeyRound, Mail, Trash2, X } from 'lucide-react';
import { Gemini } from '@lobehub/icons';
import { useTranslation } from 'react-i18next';
import type { GeminiAppAccount } from '../../stores/useGeminiAppAccounts';
import { cn } from '../../utils/cn';
import { GeminiAppDiagnostics, useGeminiAppTermsActions } from './GeminiAppTerms';

const prefix = 'accounts.add.gemini_app';

type Tone = 'ok' | 'warn' | 'error' | 'muted' | 'info';

const toneText: Record<Tone, string> = {
    ok: 'text-emerald-600 dark:text-emerald-400',
    warn: 'text-amber-600 dark:text-amber-400',
    error: 'text-rose-600 dark:text-rose-400',
    muted: 'text-gray-400 dark:text-gray-500',
    info: 'text-blue-600 dark:text-blue-400',
};
const toneBar: Record<Tone, string> = {
    ok: 'bg-emerald-500',
    warn: 'bg-amber-500',
    error: 'bg-rose-500',
    muted: 'bg-gray-300 dark:bg-gray-600',
    info: 'bg-blue-500 animate-pulse',
};

/** A status pill styled like GroupQuotaCard rows: icon + label on the left, value on the right, accent line underneath. */
function StatusRow({ icon, label, value, tone, title }: { icon: ReactNode; label: string; value: string; tone: Tone; title?: string }) {
    return (
        <div
            className={cn(
                'relative h-[22px] flex items-center px-1.5 rounded-md overflow-hidden border border-gray-100/50 dark:border-white/5 bg-gray-50/30 dark:bg-white/5',
                tone === 'error' && 'border-rose-400/70 dark:border-rose-500/70 bg-rose-50/80 dark:bg-rose-950/30',
            )}
            title={title}
        >
            <div className="absolute bottom-0 inset-x-0 h-[2.5px] bg-black/[0.05] dark:bg-white/[0.08]">
                <div className={cn('h-full w-full', toneBar[tone])} />
            </div>
            <div className="relative z-10 w-full flex items-center text-[10px] font-mono leading-none gap-1.5">
                <span className="flex-1 min-w-0 flex items-center gap-1 font-bold text-gray-700 dark:text-gray-200">
                    {icon}
                    <span className="truncate">{label}</span>
                </span>
                <span className={cn('shrink-0 max-w-[60%] truncate font-bold', toneText[tone])}>{value}</span>
            </div>
        </div>
    );
}

/** Grid card for a Gemini App account, laid out like AccountCard (header / status rows / action footer). */
export function GeminiAppAccountCard({ account, onRemove }: { account: GeminiAppAccount; onRemove: () => void }) {
    const { t } = useTranslation();
    const [panel, setPanel] = useState<'details' | 'confirm'>();
    const { pending, mayConfirm, error, clearError, confirm } = useGeminiAppTermsActions(account);
    const { terms, last_error: failure } = account;
    const hasDiagnostics = Boolean(failure || terms?.diagnostic);

    const termsState = pending ? 'pending' : terms?.status ?? 'unknown';
    const termsTone: Tone = { pending: 'info', accepted: 'ok', error: 'error', unknown: 'muted' }[termsState] as Tone;
    const failureText = failure ? [failure.model, failure.code].filter(Boolean).join(' · ') : '';

    return (
        <motion.div
            whileHover={{ y: -2 }}
            transition={{ duration: 0.15 }}
            className="relative flex flex-col p-3 rounded-xl border transition-all hover:shadow-md bg-white dark:bg-base-100 border-gray-200 dark:border-base-300"
        >
            <div className="flex-none flex items-start gap-3 mb-2">
                <Mail className="mt-0.5 h-4 w-4 shrink-0 text-gray-400" aria-hidden="true" />
                <div className="flex-1 min-w-0 flex flex-col gap-1.5">
                    <h3 className="font-semibold text-sm truncate w-full text-gray-900 dark:text-base-content" title={account.email}>
                        {account.email}
                    </h3>
                    <div className="flex items-center gap-1.5 flex-wrap">
                        <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 text-[9px] font-bold shadow-sm border border-blue-200/50">
                            <Gemini.Color size={10} />
                            Gemini App
                        </span>
                    </div>
                </div>
            </div>

            <div className="flex-1 px-1 mb-2 flex flex-col gap-1.5">
                <StatusRow
                    icon={<KeyRound className="w-3 h-3 shrink-0" />}
                    label={t(`${prefix}.card.login`)}
                    value={t(`${prefix}.card.${account.needs_login ? 'login_required' : 'login_ok'}`)}
                    tone={account.needs_login ? 'warn' : 'ok'}
                />
                <StatusRow
                    icon={<FileCheck2 className="w-3 h-3 shrink-0" />}
                    label={t(`${prefix}.card.terms`)}
                    value={t(`${prefix}.card.terms_${termsState}`)}
                    tone={termsTone}
                    title={t(`${prefix}.terms.${termsState}`)}
                />
                {failure && (
                    <StatusRow
                        icon={<AlertTriangle className="w-3 h-3 shrink-0" />}
                        label={t(`${prefix}.card.last_error`)}
                        value={failureText}
                        tone="error"
                        title={failure.diagnostic?.message ?? failureText}
                    />
                )}
                {error && <p role="alert" className="text-[10px] text-rose-600 dark:text-rose-400">{error}</p>}
            </div>

            <div className="flex-none flex items-center justify-center gap-1 pt-2 pb-1 border-t border-gray-100 dark:border-base-200">
                {hasDiagnostics && (
                    <button
                        type="button"
                        className="p-1.5 text-gray-400 hover:text-sky-600 dark:hover:text-sky-400 hover:bg-sky-50 dark:hover:bg-sky-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); setPanel('details'); }}
                        title={t(`${prefix}.card.details`)}
                        aria-label={t(`${prefix}.card.details`)}
                    >
                        <Info className="w-3.5 h-3.5" />
                    </button>
                )}
                {mayConfirm && !pending && (
                    <button
                        type="button"
                        className="p-1.5 text-gray-400 hover:text-emerald-600 dark:hover:text-emerald-400 hover:bg-emerald-50 dark:hover:bg-emerald-900/30 rounded-lg transition-all"
                        onClick={(e) => { e.stopPropagation(); clearError(); setPanel('confirm'); }}
                        title={t(`${prefix}.terms.action`)}
                        aria-label={t(`${prefix}.terms.action`)}
                    >
                        <FileCheck2 className="w-3.5 h-3.5" />
                    </button>
                )}
                <button
                    type="button"
                    className="p-1.5 text-gray-400 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-all"
                    onClick={(e) => { e.stopPropagation(); onRemove(); }}
                    title={t(`${prefix}.remove`)}
                    aria-label={t(`${prefix}.remove`)}
                >
                    <Trash2 className="w-3.5 h-3.5" />
                </button>
            </div>

            {panel && (
                <div className="absolute inset-0 z-10 flex flex-col rounded-xl bg-white/95 dark:bg-base-100/95 p-3 text-[11px] text-gray-700 dark:text-gray-200">
                    <div className="flex items-center justify-between mb-1">
                        <span className="font-semibold text-xs">{t(panel === 'details' ? `${prefix}.card.details` : `${prefix}.terms.action`)}</span>
                        <button type="button" className="p-1 text-gray-400 hover:text-gray-700 rounded-md" onClick={() => setPanel(undefined)} aria-label={t('common.cancel')}>
                            <X className="w-3.5 h-3.5" />
                        </button>
                    </div>
                    {panel === 'details' ? (
                        <div className="flex-1 overflow-y-auto min-h-0"><GeminiAppDiagnostics account={account} /></div>
                    ) : (
                        <div className="flex-1 flex flex-col justify-between gap-2">
                            <p>{t(`${prefix}.terms.confirm`, { email: account.email })}</p>
                            <div className="flex justify-end gap-2">
                                <button type="button" className="btn btn-ghost btn-xs" onClick={() => setPanel(undefined)}>{t('common.cancel')}</button>
                                <button type="button" className="btn btn-primary btn-xs" disabled={!mayConfirm} onClick={() => { setPanel(undefined); void confirm(); }}>{t(`${prefix}.terms.submit`)}</button>
                            </div>
                        </div>
                    )}
                </div>
            )}
        </motion.div>
    );
}
