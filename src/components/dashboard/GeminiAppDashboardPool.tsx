import { ArrowRight } from 'lucide-react';
import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { GeminiAppAccountInfo } from '../accounts/GeminiAppAccountInfo';
import { useGeminiAppAccounts } from '../../stores/useGeminiAppAccounts';

export function GeminiAppDashboardPool({ recommendations = false }: { recommendations?: boolean }) {
    const { t } = useTranslation();
    const accounts = useGeminiAppAccounts(state => state.accounts);
    const loadFailed = useGeminiAppAccounts(state => state.loadFailed);
    const authorized = accounts.filter(account => !account.needs_login);
    const displayed = recommendations ? authorized : accounts;
    const prefix = 'accounts.add.gemini_app';
    return <div className="flex flex-1 flex-col gap-3">
        <div className="space-y-1 text-sm">
            <h3 className="font-semibold text-gray-900 dark:text-gray-100">{t(`${prefix}.pool`)}</h3>
            <p className="text-xs text-gray-600 dark:text-gray-400">{t(`${prefix}.${recommendations ? 'no_ranking' : 'automatic_pool'}`)}</p>
        </div>
        {loadFailed ? <p role="alert" className="text-sm text-red-600 dark:text-red-400">{t(`${prefix}.errors.request_failed`)}</p>
            : displayed.length === 0 ? <p className="text-sm text-gray-500 dark:text-gray-400">{t(`${prefix}.${recommendations && accounts.length ? 'none_authorized' : 'empty'}`)}</p>
            : <ul className="max-h-36 overflow-y-auto space-y-1.5">
                {displayed.map(account => <li key={account.id} className="rounded-xl border border-gray-100 dark:border-base-300 bg-gray-50 dark:bg-base-200 px-3 py-2"><GeminiAppAccountInfo account={account} /></li>)}
            </ul>}
        <Link to="/accounts?pool=gemini-app" className="mt-auto flex items-center justify-center gap-1 rounded-lg border border-gray-200 dark:border-base-300 py-2 text-xs font-semibold text-blue-700 dark:text-blue-300 hover:bg-blue-50 dark:hover:bg-blue-900/20">
            {t(`${prefix}.manage_pool`)}<ArrowRight className="h-3.5 w-3.5" aria-hidden="true" />
        </Link>
    </div>;
}
