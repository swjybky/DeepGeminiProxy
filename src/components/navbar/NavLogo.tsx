import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import LogoIcon from '../../../src-tauri/icons/icon.png';

export function NavLogo() {
    const { t } = useTranslation();
    const appName = t('common.app_name', 'DeepGeminiProxy');

    return (
        <Link to="/" aria-label={appName} draggable="false" className="flex w-full min-w-0 items-center gap-2 text-lg font-semibold tracking-tight text-gray-900 dark:text-base-content">
            <div className="relative flex shrink-0 items-center justify-center">
                <img
                    src={LogoIcon}
                    alt=""
                    className="w-8 h-8 cursor-pointer active:scale-95 transition-transform relative z-10"
                    draggable="false"
                />
            </div>

            {/* 父容器宽度 < 200px 隐藏 */}
            <span className="hidden min-w-0 truncate @[200px]/logo:block">{appName}</span>
        </Link>
    );
}
