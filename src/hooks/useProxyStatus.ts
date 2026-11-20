import { useCallback, useEffect, useState } from 'react';
import { request } from '../utils/request';

interface ProxyStatus {
    running: boolean;
    port: number;
    base_url: string;
    lan_ip?: string | null;
    active_accounts: number;
}

export function useProxyStatus() {
    const [status, setStatus] = useState<ProxyStatus | null>();

    const refreshStatus = useCallback(async () => {
        try {
            const next = await request<ProxyStatus>('get_proxy_status');
            if (next.base_url !== 'starting' && next.base_url !== 'busy') {
                setStatus(next);
                return;
            }
        } catch (error) {
            console.error('Failed to load proxy status:', error);
        }
        setStatus(previous => previous ?? null);
    }, []);

    useEffect(() => {
        void refreshStatus();
        const interval = setInterval(refreshStatus, 3000);
        return () => clearInterval(interval);
    }, [refreshStatus]);

    return { status, refreshStatus };
}
