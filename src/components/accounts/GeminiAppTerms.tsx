import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useGeminiAppAccounts, type GeminiAppAccount, type GeminiAppDiagnostic } from '../../stores/useGeminiAppAccounts';

const prefix = 'accounts.add.gemini_app';

function Diagnostic({ details }: { details: GeminiAppDiagnostic | null }) {
    if (!details) return null;
    const status = [
        details.upstream_http_status !== null && `HTTP ${details.upstream_http_status}`,
        details.grpc_status !== null && `gRPC ${details.grpc_status}`,
    ].filter(Boolean).join(' · ');
    return <>
        {status && <p className="mt-1 opacity-75">{status}</p>}
        {details.message && <p className="mt-1 whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{details.message}</p>}
    </>;
}

/** Terms state and confirm action shared by the account list and the grid card. */
export function useGeminiAppTermsActions(account: GeminiAppAccount) {
    const { t } = useTranslation();
    const [error, setError] = useState('');
    const pending = useGeminiAppAccounts(state => state.confirmingTerms.has(account.id));
    const confirmTerms = useGeminiAppAccounts(state => state.confirmTerms);
    const { terms, last_error: failure } = account;

    const mayConfirm = Boolean(terms) && !account.needs_login && (
        terms!.status !== 'accepted' || (
            failure?.code === 'NATIVE_PRECONDITION_REQUIRED' &&
            failure.at > Date.parse(terms!.accepted_at ?? '') &&
            /accept (?:our |the )?terms of service/i.test(failure.diagnostic?.message ?? '')
        )
    );

    async function confirm() {
        setError('');
        try {
            await confirmTerms(account.id);
        } catch (err) {
            const code = typeof err === 'string' ? err : 'request_failed';
            setError(t(`${prefix}.errors.${code}`, { defaultValue: t(`${prefix}.terms.failed`) }));
        }
    }

    return { pending, mayConfirm, error, clearError: () => setError(''), confirm };
}

/** Redacted model / terms diagnostics. */
export function GeminiAppDiagnostics({ account }: { account: GeminiAppAccount }) {
    const { t } = useTranslation();
    const { terms, last_error: failure } = account;
    return <>
        {failure && <div className="mt-2">
            <p className="font-medium [overflow-wrap:anywhere]">{t(`${prefix}.terms.model_error`)}: {[failure.model, failure.code].filter(Boolean).join(' · ')}</p>
            <Diagnostic details={failure.diagnostic} />
        </div>}
        {terms?.diagnostic && <div className="mt-2">
            <p className="font-medium">{t(`${prefix}.terms.${terms.operation === 'version' ? 'version_error' : 'accept_error'}`)}</p>
            <Diagnostic details={terms.diagnostic} />
        </div>}
    </>;
}

export function GeminiAppTerms({ account, disabled = false }: { account: GeminiAppAccount; disabled?: boolean }) {
    const { t } = useTranslation();
    const [asking, setAsking] = useState(false);
    const { pending, mayConfirm, error, clearError, confirm } = useGeminiAppTermsActions(account);
    const { terms, last_error: failure } = account;
    if (!terms) return null;

    return <div className="mt-2 min-w-0 space-y-2 text-xs">
        <div className="flex flex-wrap items-center gap-2">
            <span role="status">{t(`${prefix}.terms.${pending ? 'pending' : terms.status}`)}</span>
            {mayConfirm && !asking && !pending && <button type="button" className="btn btn-ghost btn-xs" disabled={disabled} onClick={() => { clearError(); setAsking(true); }}>
                {t(`${prefix}.terms.action`)}
            </button>}
        </div>
        {asking && !pending && <div className="space-y-2 rounded-lg border border-base-300 p-2">
            <p>{t(`${prefix}.terms.confirm`, { email: account.email })}</p>
            <div className="flex flex-wrap gap-2">
                <button type="button" className="btn btn-primary btn-xs" disabled={disabled || !mayConfirm} onClick={() => { setAsking(false); void confirm(); }}>{t(`${prefix}.terms.submit`)}</button>
                <button type="button" className="btn btn-ghost btn-xs" onClick={() => setAsking(false)}>{t('common.cancel')}</button>
            </div>
        </div>}
        {error && <p role="alert" className="text-red-700 dark:text-red-300">{error}</p>}
        {(failure || terms.diagnostic) && <details className="max-w-full">
            <summary className="cursor-pointer">{t(`${prefix}.terms.details`)}</summary>
            <GeminiAppDiagnostics account={account} />
        </details>}
    </div>;
}
