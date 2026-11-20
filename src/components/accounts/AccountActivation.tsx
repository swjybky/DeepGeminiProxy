import { ArrowRight, ArrowRightLeft, Check, Power, Repeat2, Terminal } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { Account, ActiveClientsStatus, AntigravityClient } from '../../types/account';
import ModalDialog from '../common/ModalDialog';

const clients = [
    { id: 'classic', label: 'client_classic', icon: ArrowRightLeft, color: 'bg-blue-100 text-blue-700 dark:bg-blue-900/50 dark:text-blue-300' },
    { id: 'ide', label: 'client_ide', icon: Repeat2, color: 'bg-sky-100 text-sky-700 dark:bg-sky-900/50 dark:text-sky-300' },
    { id: 'agy', label: 'client_cli', icon: Terminal, color: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/50 dark:text-emerald-300' },
] as const;

export function ActiveClientBadges({ accountId, status }: { accountId: string; status?: ActiveClientsStatus }) {
    const { t } = useTranslation();
    return clients.map(({ id, label, color }) => status?.[`${id}_account_id`] === accountId && (
        <span
            key={id}
            className={`inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10px] font-semibold ${color}`}
            title={t(`accounts.${label}_tooltip`)}
        >
            <Check className="w-2.5 h-2.5" aria-hidden="true" />
            {t(`accounts.${label}`)}
        </span>
    ));
}

export function AccountActivationButton({ onClick, disabled }: { onClick: () => void; disabled: boolean }) {
    const { t } = useTranslation();
    return (
        <button
            type="button"
            onClick={(event) => { event.stopPropagation(); onClick(); }}
            disabled={disabled}
            aria-haspopup="dialog"
            title={t('accounts.activation.title')}
            className="inline-flex items-center gap-1 rounded-lg px-1.5 py-1.5 text-xs font-medium text-blue-600 hover:bg-blue-50 dark:text-blue-400 dark:hover:bg-blue-900/30 disabled:opacity-50"
        >
            <Power className="w-3.5 h-3.5" aria-hidden="true" />
            {t('common.enable')}
        </button>
    );
}

interface AccountActivationDialogProps {
    account: Account;
    accounts: Account[];
    status: ActiveClientsStatus;
    busy: boolean;
    onLaunch: (client: AntigravityClient) => void;
    onClose: () => void;
}

export function AccountActivationDialog({ account, accounts, status, busy, onLaunch, onClose }: AccountActivationDialogProps) {
    const { t } = useTranslation();
    return (
        <ModalDialog
            isOpen
            title={t('accounts.activation.title')}
            type="info"
            onConfirm={onClose}
            confirmText={t('common.close')}
            isLoading={busy}
        >
            <p className="mb-3 break-all text-center text-sm text-gray-600 dark:text-gray-300">{account.email}</p>
            <div className="grid gap-2">
                {clients.map(({ id, label, icon: Icon, color }) => {
                    const currentId = status[`${id}_account_id`];
                    const currentEmail = status[`${id}_email`] || accounts.find(({ id }) => id === currentId)?.email;
                    const isCurrent = currentId === account.id;
                    return (
                        <button
                            key={id}
                            type="button"
                            disabled={busy || account.disabled}
                            className="flex w-full items-center gap-3 rounded-xl border border-gray-200 px-3 py-2 text-left transition-colors hover:border-blue-300 hover:bg-blue-50 focus-visible:ring-2 focus-visible:ring-blue-500 disabled:opacity-50 dark:border-base-300 dark:hover:border-blue-700 dark:hover:bg-blue-900/20"
                            onClick={() => onLaunch(id)}
                        >
                            <span className={`shrink-0 rounded-lg p-2 ${color}`}><Icon className="w-4 h-4" aria-hidden="true" /></span>
                            <span className="min-w-0 flex-1">
                                <span className="block text-sm font-medium">{t(`accounts.${label}`)}</span>
                                <span className="block break-all text-xs text-gray-500 dark:text-gray-400">
                                    {isCurrent
                                        ? t('accounts.activation.current_account')
                                        : currentEmail
                                            ? t('accounts.activation.signed_in_as', { email: currentEmail })
                                            : t('accounts.activation.no_account')}
                                </span>
                            </span>
                            <ArrowRight className="w-4 h-4 shrink-0 text-gray-400" aria-hidden="true" />
                        </button>
                    );
                })}
            </div>
        </ModalDialog>
    );
}
