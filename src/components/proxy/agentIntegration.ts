import { isTauri } from '../../utils/env';
import { request } from '../../utils/request';

export const agentApps = [
    'DeepWrite', 'Claude', 'DeepSeekHarness', 'Codex', 'OpenCode', 'GrokBuild', 'Hermes', 'OpenClaw', 'Gemini', 'Droid',
] as const;

export type AgentApp = typeof agentApps[number];

export interface AgentStatus {
    installed: boolean;
    version: string | null;
    is_synced: boolean;
    current_base_url: string | null;
    current_model?: string | null;
    default_model?: string | null;
    detected_version_target?: string | null;
    synced_version?: string | null;
    restore_on_disconnect?: boolean;
    integration_enabled?: boolean;
    files: string[];
}

interface AgentModel {
    id: string;
    name: string;
}

const providerCommands = {
    OpenCode: 'opencode',
    Hermes: 'hermes',
    OpenClaw: 'openclaw',
    Droid: 'droid',
} as const;

const defaultModels = {
    Claude: 'claude-sonnet-4-6',
    Codex: 'gpt-oss-120b-medium',
    Gemini: 'gemini-3.1-pro-high',
    GrokBuild: 'gemini-3.8-flash-high',
    DeepSeekHarness: 'gemini-3.8-flash-high',
} as const;

function endpoint(app: AgentApp, proxyUrl: string) {
    const base = proxyUrl.trim().replace(/\/+$/, '').replace(/\/v1$/, '');
    return app === 'Claude' || app === 'Gemini' || app === 'Droid' ? base : `${base}/v1`;
}

function providerFor(app: AgentApp) {
    return app in providerCommands ? providerCommands[app as keyof typeof providerCommands] : undefined;
}

export async function getAgentStatus(app: AgentApp, proxyUrl: string): Promise<AgentStatus> {
    const provider = providerFor(app);
    const url = endpoint(app, proxyUrl);
    if (app === 'DeepWrite') {
        if (!isTauri()) return { installed: false, version: null, is_synced: false, current_base_url: null, files: [] };
        return request<AgentStatus>('get_deepwrite_sync_status', { proxyUrl: url });
    }
    if (app === 'OpenClaw') {
        const status = await request<{
            installed: boolean;
            version: string | null;
            isSynced: boolean;
            currentBaseUrl: string | null;
            defaultModel: string | null;
            detectedVersionTarget: string | null;
            syncedVersion: string | null;
            files: string[];
        }>('get_openclaw_sync_status', { proxyUrl: url });
        return {
            installed: status.installed,
            version: status.version,
            is_synced: status.isSynced,
            current_base_url: status.currentBaseUrl,
            default_model: status.defaultModel,
            detected_version_target: status.detectedVersionTarget,
            synced_version: status.syncedVersion,
            files: status.files,
        };
    }
    if (provider) return request<AgentStatus>(`get_${provider}_sync_status`, { proxyUrl: url });

    const status = await request<AgentStatus>('get_cli_sync_status', { appType: app, proxyUrl: url });
    return { ...status, restore_on_disconnect: true };
}

export function getConfigContent(app: AgentApp, fileName?: string): Promise<string> {
    const provider = providerFor(app);
    if (app === 'OpenCode') return request('get_opencode_config_content', { request: { fileName } });
    if (provider) return request(`get_${provider}_config_content`, {});
    return request('get_cli_config_content', { appType: app, fileName });
}

export async function configureAgent(
    app: AgentApp,
    proxyUrl: string,
    apiKey: string,
    models: AgentModel[],
    status?: AgentStatus,
) {
    if (app === 'DeepWrite') return request('execute_deepwrite_sync');
    const modelIds = models.map(model => model.id);
    const url = endpoint(app, proxyUrl);
    if (app === 'OpenCode') {
        return request('execute_opencode_sync', {
            proxyUrl: url, apiKey, syncAccounts: false,
            models: models.length ? models.map(({ id, name }) => ({ id, name })) : undefined,
        });
    }
    if (app === 'Hermes') {
        return request('execute_hermes_sync', {
            proxyUrl: url, apiKey, discoverModels: true, models: [], activate: false,
        });
    }

    if (app === 'OpenClaw' || app === 'Droid') {
        if (!models.length) throw new Error('No available gateway models');
        if (app === 'OpenClaw') {
            return request('execute_openclaw_sync', {
                proxyUrl: url, apiKey, models: modelIds, activate: false,
                targetVersion: status?.detected_version_target ?? status?.synced_version ?? 'v2',
            });
        }

        return request('execute_droid_sync', {
            managedOnly: true,
            customModels: models.map(({ id, name }) => {
                const anthropic = id.startsWith('claude-');
                const displayName = `AG-${name}`;
                return {
                    id: `custom:AG-${id}`,
                    model: id,
                    displayName,
                    baseUrl: anthropic ? url : `${url}/v1`,
                    apiKey,
                    provider: anthropic ? 'anthropic' : 'generic-chat-completion-api',
                    noImageSupport: !(anthropic || /^(gemini-|gpt-4)|image/.test(id)),
                    ...(anthropic ? { maxOutputTokens: 64000 } : {}),
                    ...(anthropic && id.includes('thinking')
                        ? { extraArgs: { thinking: { type: 'enabled', budget_tokens: 32000 } } }
                        : {}),
                };
            }),
        });
    }

    const preferred = defaultModels[app];
    const chatModels = modelIds.filter(id => !id.includes('image'));
    const preferredFamily = preferred.split('-')[0] + '-';
    const model = chatModels.includes(preferred) ? preferred
        : chatModels.find(id => id.startsWith(preferredFamily)) ?? chatModels[0] ?? preferred;
    return request('execute_cli_sync', { appType: app, proxyUrl: url, apiKey, model, integration: true });
}

export function disconnectAgent(app: AgentApp) {
    if (app === 'DeepWrite') return request('execute_deepwrite_clear');
    if (app === 'Droid') {
        return request('execute_droid_sync', { managedOnly: true, customModels: [] });
    }
    if (app === 'OpenCode') {
        return request('execute_opencode_clear', { providerOnly: true });
    }
    if (app === 'Hermes' || app === 'OpenClaw') {
        return request(`execute_${providerCommands[app]}_clear`, {});
    }
    return request('execute_cli_restore', { appType: app });
}
