import { currentMonitor, getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { isTauri } from './env';

export const MINI_MODE_WIDTH = 360;
export const MINI_MODE_HEIGHT = 360;

// Match the main window defaults in tauri.conf.json (logical pixels).
const MAIN_WINDOW_SIZE = new LogicalSize(1200, 800);
const MIN_STARTUP_SIZE = new LogicalSize(900, 600);

/**
 * Enter mini view mode
 * @param contentHeight The height of the content to fit (optional, defaults to MINI_MODE_HEIGHT)
 * @param shouldCenter Whether to center the window (default: false)
 */
export const enterMiniMode = async (contentHeight?: number, shouldCenter: boolean = false, isPinned: boolean = false) => {
    if (!isTauri()) return;
    try {
        const win = getCurrentWindow();

        // Hide window decorations (title bar) first to ensure accurate sizing
        await win.setDecorations(false);

        // Enforce fixed, stable window size to prevent jumping/stretching during tab or account switches
        const targetHeight = contentHeight && contentHeight > 0 ? contentHeight : MINI_MODE_HEIGHT;
        await win.setSize(new LogicalSize(MINI_MODE_WIDTH, targetHeight));

        await win.setAlwaysOnTop(isPinned);
        // Enable window shadow
        await win.setShadow(true);
        // Disable resizing in mini mode
        await win.setResizable(false);

        // Center window only if requested (usually on first load)
        if (shouldCenter) {
            await win.center();
        }
    } catch (error) {
        console.error('Failed to enter mini mode:', error);
    }
};

/**
 * Exit mini view mode and restore default window state
 */
export const exitMiniMode = async () => {
    if (!isTauri()) return;
    try {
        const win = getCurrentWindow();
        // Restore to a reasonable default size
        await win.setSize(MAIN_WINDOW_SIZE);
        await win.setAlwaysOnTop(false);
        await win.center();
        // Restore window decorations (title bar)
        await win.setDecorations(true);
        // Re-enable resizing
        await win.setResizable(true);
    } catch (error) {
        console.error('Failed to exit mini mode:', error);
    }
};

/** Repair an undersized saved main window once per launch, not during manual resizing. */
export const ensureFullViewState = async () => {
    if (!isTauri()) return;
    try {
        const win = getCurrentWindow();
        await win.setDecorations(true);
        await win.setResizable(true);
        await win.setAlwaysOnTop(false);

        if (await win.isFullscreen() || await win.isMaximized()) return;

        const [physicalSize, scaleFactor] = await Promise.all([win.innerSize(), win.scaleFactor()]);
        // Tauri restores physical pixels; on Retina, 1024×700 can be just 512×350 logical pixels.
        const size = physicalSize.toLogical(scaleFactor);
        if (size.width >= MIN_STARTUP_SIZE.width && size.height >= MIN_STARTUP_SIZE.height) return;

        const monitor = await currentMonitor();
        const workArea = monitor?.workArea.size.toLogical(monitor.scaleFactor);
        const width = Math.min(MAIN_WINDOW_SIZE.width, workArea?.width || MAIN_WINDOW_SIZE.width);
        const height = Math.min(MAIN_WINDOW_SIZE.height, workArea?.height || MAIN_WINDOW_SIZE.height);
        if (size.width < Math.min(MIN_STARTUP_SIZE.width, width) ||
            size.height < Math.min(MIN_STARTUP_SIZE.height, height)) {
            await win.setSize(new LogicalSize(width, height));
            await win.center();
        }
    } catch (error) {
        console.error('Failed to ensure full view state:', error);
    }
};
