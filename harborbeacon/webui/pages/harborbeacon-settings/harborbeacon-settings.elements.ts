import { marker as T } from '@biesbjerg/ngx-translate-extract-marker';

import { HarborBeaconSettingsComponent } from './harborbeacon-settings.component';

/**
 * Route element definition for the HarborBeacon Settings page.
 *
 * Register in the HarborOS WebUI sidebar under "System" or as a
 * top-level icon.  Example integration in app-routing:
 *
 *   {
 *     path: 'harborbeacon',
 *     loadComponent: () =>
 *       import('./pages/harborbeacon-settings/harborbeacon-settings.component')
 *         .then(m => m.HarborBeaconSettingsComponent),
 *     data: { title: T('HarborBeacon'), breadcrumb: T('HarborBeacon') },
 *   }
 */
export const harborBeaconSettingsElements = {
  harborbeacon: {
    title: T('HarborBeacon'),
    breadcrumb: T('HarborBeacon'),
    component: HarborBeaconSettingsComponent,
    icon: 'smart_toy',
  },
};
