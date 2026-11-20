import { Mail, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { GeminiAppAccount } from '../../stores/useGeminiAppAccounts';

export function GeminiAppAccountInfo({ account, onRemove }: { account: GeminiAppAccount; onRemove?: () => void }) {
    const { t } = useTranslation();
    const prefix = 'accounts.add.gemini_app';
    return <div className="flex items-center gap-3 justify-between min-w-0">
        <div className="min-w-0 space-y-1">
            <div className="flex min-w-0 items-center gap-2 text-sm font-medium text-gray-800 dark:text-gray-200">
                <Mail className="h-3.5 w-3.5 shrink-0 text-gray-400" aria-hidden="true" />
                <span className="truncate" title={account.email}>{account.email}</span>
            </div>
            <div className="flex flex-wrap items-center gap-2 text-xs">
                <span className="rounded-md bg-blue-50 dark:bg-blue-500/15 px-1.5 py-0.5 font-semibold text-blue-700 dark:text-blue-300">Gemini App</span>
                <span className={account.needs_login ? 'text-amber-700 dark:text-amber-300' : 'text-emerald-700 dark:text-emerald-300'}>{t(`${prefix}.${account.needs_login ? 'needs_login' : 'authorized'}`)}</span>
            </div>
        </div>
        {onRemove && <button type="button" aria-label={t(`${prefix}.remove`)} title={t(`${prefix}.remove`)} className="shrink-0 rounded-lg p-2 text-gray-500 hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-900/25" onClick={onRemove}><Trash2 className="h-4 w-4" aria-hidden="true" /></button>}
    </div>;
}
