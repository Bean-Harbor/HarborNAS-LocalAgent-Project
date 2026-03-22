import { marker as T } from '@biesbjerg/ngx-translate-extract-marker';

import { ExtensionManagerComponent } from './extension-manager.component';

/**
 * Route element definition for the Extension Manager page.
 *
 * Register in the HarborOS WebUI sidebar under "System" or "HarborClaw".
 *
 *   {
 *     path: 'extensions',
 *     loadComponent: () =>
 *       import('./pages/extension-manager/extension-manager.component')
 *         .then(m => m.ExtensionManagerComponent),
 *     data: { title: T('Extensions'), breadcrumb: T('Extensions') },
 *   }
 */
export const extensionManagerElements = {
  extensions: {
    title: T('Extensions'),
    breadcrumb: T('Extensions'),
    component: ExtensionManagerComponent,
    icon: 'extension',
  },
};
