import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { MatTableModule } from '@angular/material/table';
import { MatCardModule } from '@angular/material/card';
import { MatChipsModule } from '@angular/material/chips';
import { MatIconModule } from '@angular/material/icon';
import { MatButtonModule } from '@angular/material/button';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { MatTooltipModule } from '@angular/material/tooltip';
import { MatBadgeModule } from '@angular/material/badge';

import {
  ExtensionSummary,
  ExtensionType,
  ExtensionFilter,
  RiskLevel,
  EXTENSION_TYPE_META,
  ExtensionTypeMeta,
} from '../../../../interfaces/extension.interface';

@Component({
  selector: 'ix-extension-list',
  standalone: true,
  imports: [
    CommonModule,
    FormsModule,
    MatTableModule,
    MatCardModule,
    MatChipsModule,
    MatIconModule,
    MatButtonModule,
    MatSlideToggleModule,
    MatFormFieldModule,
    MatInputModule,
    MatSelectModule,
    MatTooltipModule,
    MatBadgeModule,
  ],
  templateUrl: './extension-list.component.html',
  styleUrls: ['./extension-list.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ExtensionListComponent {
  @Input() extensions: ExtensionSummary[] = [];
  @Output() selectExtension = new EventEmitter<string>();
  @Output() toggleEnabled = new EventEmitter<{ id: string; enabled: boolean }>();
  @Output() deleteExtension = new EventEmitter<string>();
  @Output() importExtension = new EventEmitter<void>();

  readonly typeMeta = EXTENSION_TYPE_META;
  readonly allTypes = Object.values(ExtensionType);
  readonly allRisks = Object.values(RiskLevel);
  readonly displayedColumns = ['name', 'type', 'version', 'capabilities', 'risk', 'enabled', 'actions'];

  filter: ExtensionFilter = {
    search: '',
    types: [],
    riskLevels: [],
    enabledOnly: false,
  };

  viewMode: 'table' | 'cards' = 'cards';

  get filtered(): ExtensionSummary[] {
    return this.extensions.filter(ext => {
      if (this.filter.search) {
        const q = this.filter.search.toLowerCase();
        const haystack = `${ext.id} ${ext.name} ${ext.summary} ${ext.capabilities.join(' ')}`.toLowerCase();
        if (!haystack.includes(q)) return false;
      }
      if (this.filter.types.length && !this.filter.types.includes(ext.type)) return false;
      if (this.filter.riskLevels.length && !this.filter.riskLevels.includes(ext.risk_level)) return false;
      if (this.filter.enabledOnly && !ext.enabled) return false;
      return true;
    });
  }

  getTypeMeta(type: ExtensionType): ExtensionTypeMeta {
    return this.typeMeta.find(m => m.type === type) ?? this.typeMeta[0];
  }

  onToggle(ext: ExtensionSummary): void {
    this.toggleEnabled.emit({ id: ext.id, enabled: !ext.enabled });
  }

  onSelect(ext: ExtensionSummary): void {
    this.selectExtension.emit(ext.id);
  }

  onDelete(ext: ExtensionSummary, event: Event): void {
    event.stopPropagation();
    this.deleteExtension.emit(ext.id);
  }

  onImport(): void {
    this.importExtension.emit();
  }
}
