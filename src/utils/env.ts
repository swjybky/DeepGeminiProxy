/**
 * Detect if the app is running in a Tauri environment
 */
export const isTauri = () => {
    return typeof window !== 'undefined' &&
        (!!(window as any).__TAURI_INTERNALS__ || !!(window as any).__TAURI__);
};

/** Keep in sync with the pre-paint check in index.html. */
export function isMiniWindow(): boolean {
    if (typeof window === 'undefined') return false;

    // Native window identity wins over any query string used for page routing.
    const label = (window as any).__TAURI_INTERNALS__?.metadata?.currentWindow?.label;
    if (typeof label === 'string') return label === 'mini';

    return new URLSearchParams(window.location.search).get('mini') === '1';
}

/**
 * Detect if running on Linux
 */
export const isLinux = () => {
    return navigator.userAgent.toLowerCase().includes('linux');
};

/**
 * Detect if running on macOS
 */
export const isMac = () => {
    return typeof navigator !== 'undefined' && /mac/i.test(navigator.userAgent);
};
