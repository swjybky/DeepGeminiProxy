import { create } from 'zustand';
import { request } from '../utils/request';

export interface GeminiAppAccount {
    id: string;
    email: string;
    needs_login: boolean;
}

interface GeminiAppAccountsState {
    accounts: GeminiAppAccount[];
    loadFailed: boolean;
    refresh: () => Promise<void>;
    remove: (id: string) => Promise<void>;
}

// Keep the App pool separate from Antigravity actions and DTOs.
export const useGeminiAppAccounts = create<GeminiAppAccountsState>(set => {
    let generation = 0;
    return {
        accounts: [],
        loadFailed: false,
        refresh: async () => {
            const version = ++generation;
            try {
                const accounts = await request<GeminiAppAccount[]>('gemini_app_accounts');
                if (version === generation) set({ accounts, loadFailed: false });
            } catch (error) {
                if (version === generation) set({ loadFailed: true });
                throw error;
            }
        },
        remove: async id => {
            await request('gemini_app_account_remove', { id });
            ++generation;
            set(state => ({ accounts: state.accounts.filter(account => account.id !== id) }));

        },
    };
});
