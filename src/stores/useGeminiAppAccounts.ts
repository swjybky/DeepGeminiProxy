import { create } from 'zustand';
import { request } from '../utils/request';

export interface GeminiAppDiagnostic {
    upstream_http_status: number | null;
    grpc_status: number | null;
    message: string | null;
}

export interface GeminiAppAccount {
    id: string;
    email: string;
    needs_login: boolean;
    terms?: {
        status: 'unknown' | 'accepted' | 'error';
        version: number | null;
        accepted_at: string | null;
        operation: 'version' | 'accept' | null;
        diagnostic: GeminiAppDiagnostic | null;
    };
    last_error?: {
        code: string;
        model: string | null;
        at: number;
        diagnostic: GeminiAppDiagnostic | null;
    };
}

interface GeminiAppAccountsState {
    accounts: GeminiAppAccount[];
    loadFailed: boolean;
    refresh: () => Promise<void>;
    remove: (id: string) => Promise<void>;
    confirmingTerms: ReadonlySet<string>;
    confirmTerms: (id: string) => Promise<void>;
}

// Keep the App pool separate from Antigravity actions and DTOs.
export const useGeminiAppAccounts = create<GeminiAppAccountsState>((set, get) => {
    let generation = 0;
    return {
        accounts: [],
        loadFailed: false,
        confirmingTerms: new Set(),
        confirmTerms: async id => {
            if (get().confirmingTerms.has(id)) return;
            set(state => ({ confirmingTerms: new Set(state.confirmingTerms).add(id) }));
            try {
                const account = await request<GeminiAppAccount>('gemini_app_confirm_terms', { id, confirmed: true });
                ++generation;
                set(state => ({ accounts: state.accounts.map(existing => existing.id === id ? account : existing) }));
            } catch (error) {
                // App2API persists any redacted terms failure separately from the last model error.
                await get().refresh().catch(() => {});
                throw error;
            } finally {
                set(state => ({ confirmingTerms: new Set([...state.confirmingTerms].filter(current => current !== id)) }));
            }
        },
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
