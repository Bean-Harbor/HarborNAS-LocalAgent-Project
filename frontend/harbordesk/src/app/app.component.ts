import { Component } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';

import { localizedHarborDeskPages } from './core/page-registry';
import { prefersChineseUi, uiText } from './core/ui-locale';

@Component({
  selector: 'hd-root',
  standalone: true,
  imports: [RouterLink, RouterLinkActive, RouterOutlet],
  templateUrl: './app.component.html',
  styleUrl: './app.component.css'
})
export class AppComponent {
  protected readonly isChineseUi = prefersChineseUi();
  protected readonly pages = localizedHarborDeskPages();

  constructor() {
    document.documentElement.lang = this.isChineseUi ? 'zh-CN' : 'en';
  }

  protected text(english: string, chinese: string): string {
    return uiText(english, chinese);
  }
}
