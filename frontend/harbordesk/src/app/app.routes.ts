import { Routes } from '@angular/router';

import { HARBORDESK_PAGES } from './core/page-registry';
import { DeskPageComponent } from './pages/desk-page.component';

export const routes: Routes = [
  {
    path: '',
    pathMatch: 'full',
    redirectTo: HARBORDESK_PAGES[0].path
  },
  ...HARBORDESK_PAGES.map((page) => ({
    path: page.path,
    component: DeskPageComponent,
    data: {
      pageId: page.id
    }
  })),
  {
    path: '**',
    redirectTo: HARBORDESK_PAGES[0].path
  }
];
