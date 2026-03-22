import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Output,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { MatDialogModule } from '@angular/material/dialog';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatTabsModule } from '@angular/material/tabs';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { MatListModule } from '@angular/material/list';

import { ValidationResult } from '../../../../interfaces/extension.interface';

@Component({
  selector: 'ix-extension-import',
  standalone: true,
  imports: [
    CommonModule,
    FormsModule,
    MatDialogModule,
    MatFormFieldModule,
    MatInputModule,
    MatButtonModule,
    MatIconModule,
    MatTabsModule,
    MatProgressBarModule,
    MatListModule,
  ],
  templateUrl: './extension-import.component.html',
  styleUrls: ['./extension-import.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ExtensionImportComponent {
  @Output() import = new EventEmitter<Record<string, unknown>>();
  @Output() validate = new EventEmitter<string>();
  @Output() cancel = new EventEmitter<void>();

  yamlText = '';
  validationResult: ValidationResult | null = null;
  importing = false;
  fileName = '';

  onFileSelected(event: Event): void {
    const input = event.target as HTMLInputElement;
    if (!input.files?.length) return;
    const file = input.files[0];
    this.fileName = file.name;
    const reader = new FileReader();
    reader.onload = () => {
      this.yamlText = reader.result as string;
    };
    reader.readAsText(file);
  }

  onValidate(): void {
    this.validate.emit(this.yamlText);
  }

  onImport(): void {
    this.importing = true;
    this.import.emit({ _yaml: this.yamlText });
  }

  onCancel(): void {
    this.cancel.emit();
  }

  get canImport(): boolean {
    return this.yamlText.trim().length > 0 && (this.validationResult?.valid ?? false);
  }
}
