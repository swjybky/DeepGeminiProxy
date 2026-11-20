import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';
import { RefreshCw, Settings } from 'lucide-react';
import { motion } from 'framer-motion';
import { CliSyncCard } from '../components/proxy/CliSyncCard';
import { useProxyStatus } from '../hooks/useProxyStatus';
import { loadConfig } from '../services/configService';
import type { ProxyConfig } from '../types/config';

export default function AgentTools() {
    const { t } = useTranslation();
    const { status } = useProxyStatus();
    const [proxyConfig, setProxyConfig] = useState<ProxyConfig | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [loadAttempt, setLoadAttempt] = useState(0);

    useEffect(() => {
        let cancelled = false;
        loadConfig().then(config => {
            if (!config?.proxy) throw new Error('Missing proxy configuration');
            if (!cancelled) setProxyConfig(config.proxy);
        }).catch(error => {
            if (!cancelled) setError(String(error));
        });
        return () => { cancelled = true; };
    }, [loadAttempt]);

    const retryLoadConfig = () => {
        setError(null);
        setLoadAttempt(attempt => attempt + 1);
    };

    const proxyUrl = status?.running && status.base_url
        ? status.base_url
        : `http://127.0.0.1:${proxyConfig?.port || 8045}`;

    return (
        <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.2, ease: 'easeOut' }}
            className="h-full w-full overflow-y-auto overflow-x-hidden"
        >
            <div className="max-w-7xl mx-auto p-5">
                {error !== null ? (
                    <div className="flex flex-col items-center gap-3 py-20 text-center">
                        <h2 className="text-lg font-semibold">{t('proxy.error.load_failed')}</h2>
                        <p className="text-sm text-gray-500 dark:text-gray-400" role="alert">{error}</p>
                        <button type="button" className="btn btn-primary btn-sm" onClick={retryLoadConfig}>
                            <RefreshCw size={14} />
                            {t('common.retry')}
                        </button>
                    </div>
                ) : !proxyConfig || status === undefined ? (
                    <div className="flex items-center justify-center gap-2 py-20 text-sm text-gray-500 dark:text-gray-400" role="status">
                        <RefreshCw size={16} className="animate-spin text-blue-500" />
                        {t('common.loading')}
                    </div>
                ) : (
                    <div className="space-y-3">
                        {(!status?.running || !proxyConfig.api_key?.trim()) && (
                            <div className="flex flex-wrap items-center justify-between gap-2 px-1">
                                <p className="text-xs text-gray-500 dark:text-gray-400">
                                    {t(proxyConfig.api_key?.trim()
                                        ? 'proxy.cli_sync.integration.gateway_stopped'
                                        : 'proxy.cli_sync.integration.key_required')}
                                </p>
                                <Link to="/api-proxy" className="btn btn-sm btn-ghost">
                                    <Settings size={14} />
                                    {t('proxy.config.title')}
                                </Link>
                            </div>
                        )}
                        <CliSyncCard
                            key={proxyUrl}
                            className="rounded-xl border border-gray-200/60 bg-gray-50/60 p-3 dark:border-base-300 dark:bg-base-200/50"
                            proxyUrl={proxyUrl}
                            apiKey={proxyConfig.api_key ?? ''}
                        />
                    </div>
                )}
            </div>
        </motion.div>
    );
}
