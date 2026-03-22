import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
  OnChanges,
  SimpleChanges,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { MatTabsModule } from '@angular/material/tabs';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { MatChipsModule } from '@angular/material/chips';
import { MatIconModule } from '@angular/material/icon';
import { MatButtonModule } from '@angular/material/button';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatDividerModule } from '@angular/material/divider';

import {
  ExtensionDetail,
  ExtensionType,
  RiskLevel,
  EXTENSION_TYPE_META,
} from '../../../../interfaces/extension.interface';

@Component({
  selector: 'ix-extension-detail',
  standalone: true,
  imports: [
    CommonModule,
    FormsModule,
    MatTabsModule,
    MatFormFieldModule,
    MatInputModule,
    MatSelectModule,
    MatChipsModule,
    MatIconModule,
    MatButtonModule,
    MatSlideToggleModule,
    MatDividerModule,
  ],
  templateUrl: './extension-detail.component.html',
  styleUrls: ['./extension-detail.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ExtensionDetailComponent implements OnChanges {
  @Input() extension: ExtensionDetail | null = null;
  @Output() save = new EventEmitter<ExtensionDetail>();
  @Output() back = new EventEmitter<void>();

  draft: ExtensionDetail | null = null;
  yamlSource = '';
  dirty = false;

  readonly types = Object.values(ExtensionType);
  readonly risks = Object.values(RiskLevel);
  readonly typeMeta = EXTENSION_TYPE_META;
  readonly executorKeys = ['cli', 'browser', 'mcp'];

  capabilitiesText = '';

  ngOnChanges(changes: SimpleChanges): void {
    if (changes['extension'] && this.extension) {
      this.draft = structuredClone(this.extension);
      this.capabilitiesText = this.draft.capabilities.join(', ');
      this.yamlSource = this.toYaml(this.draft);
      this.dirty = false;
    }
  }

  markDirty(): void {
    this.dirty = true;
  }

  onCapabilitiesChange(text: string): void {
    if (this.draft) {
      this.draft.capabilities = text.split(',').map(s => s.trim()).filter(Boolean);
      this.dirty = true;
    }
  }

  onYamlChange(yaml: string): void {
    this.yamlSource = yaml;
    this.dirty = true;
  }

  onSave(): void {
    if (this.draft) {
      this.save.emit(structuredClone(this.draft));
    }
  }

  onBack(): void {
    this.back.emit();
  }

  private toYaml(ext: ExtensionDetail): string {
    const lines: string[] = [
      `id: ${ext.id}`,
      `name: ${ext.name}`,
      `type: ${ext.type}`,
      `version: ${ext.version}`,
      `summary: ${ext.summary}`,
      `owner: ${ext.owner}`,
      `capabilities:`,
      ...ext.capabilities.map(c => `  - ${c}`),
    ];
    if (ext.harbor_api?.enabled) {
      lines.push('harbor_api:', `  enabled: true`, `  endpoint_group: ${ext.harbor_api.endpoint_group}`);
    }
    if (ext.harbor_cli?.enabled) {
      lines.push('harbor_cli:', `  enabled: true`, `  command_group: ${ext.harbor_cli.command_group}`);
    }
    lines.push(`risk:`, `  default_level: ${ext.risk.default_level}`);
    return lines.join('\n');
  }
}
