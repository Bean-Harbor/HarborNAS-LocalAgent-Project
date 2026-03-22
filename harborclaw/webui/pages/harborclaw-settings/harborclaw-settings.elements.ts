import { marker as T } from '@biesbjerg/ngx-translate-extract-marker';

import { HarborClawSettingsComponent } from './harborclaw-settings.component';

/**
 * Route element definition for the HarborClaw Settings page.
 *
 * Register in the HarborOS WebUI sidebar under "System" or as a
 * top-level icon.  Example integration in app-routing:
 *
 *   {
 *     path: 'harborclaw',
 *     loadComponent: () =>
 *       import('./pages/harborclaw-settings/harborclaw-settings.component')
 *         .then(m => m.HarborClawSettingsComponent),
 *     data: { title: T('HarborClaw'), breadcrumb: T('HarborClaw') },
 *   }
 */
export const harborClawSettingsElements = {
  harborclaw: {
    title: T('HarborClaw'),
    breadcrumb: T('HarborClaw'),
    component: HarborClawSettingsComponent,
    icon: 'smart_toy',
  },
};
