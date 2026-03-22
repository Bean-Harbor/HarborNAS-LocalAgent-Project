import {
  ChangeDetectionStrategy,
  ChangeDetectorRef,
  Component,
  OnInit,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { MatIconModule } from '@angular/material/icon';
import { MatButtonModule } from '@angular/material/button';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { MatProgressBarModule } from '@angular/material/progress-bar';

import {
  ExtensionSummary,
  ExtensionDetail,
  ValidationResult,
} from '../../interfaces/extension.interface';

import { ExtensionService } from '../../services/extension.service';
import { ExtensionListComponent } from './components/extension-list/extension-list.component';
import { ExtensionDetailComponent } from './components/extension-detail/extension-detail.component';
import { ExtensionImportComponent } from './components/extension-import/extension-import.component';

type ViewMode = 'list' | 'detail' | 'import';

@Component({
  selector: 'ix-extension-manager',
  standalone: true,
  imports: [
    CommonModule,
    MatIconModule,
    MatButtonModule,
    MatSnackBarModule,
    MatProgressBarModule,
    ExtensionListComponent,
    ExtensionDetailComponent,
    ExtensionImportComponent,
  ],
  templateUrl: './extension-manager.component.html',
  styleUrls: ['./extension-manager.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ExtensionManagerComponent implements OnInit {
  // ---- state ----
  extensions: ExtensionSummary[] = [];
  selectedExtension: ExtensionDetail | null = null;
  validationResult: ValidationResult | null = null;
  viewMode: ViewMode = 'list';
  loading = true;

  constructor(
    private extensionService: ExtensionService,
    private snackBar: MatSnackBar,
    private cdr: ChangeDetectorRef,
  ) {}

  ngOnInit(): void {
    this.loadExtensions();
  }

  // ---- data flow ----

  loadExtensions(): void {
    this.loading = true;
    this.cdr.markForCheck();
    this.extensionService.listExtensions().subscribe({
      next: (list) => {
        this.extensions = list;
        this.loading = false;
        this.cdr.markForCheck();
      },
      error: (err) => {
        this.snackBar.open('Failed to load extensions', 'Close', { duration: 5000 });
        this.loading = false;
        this.cdr.markForCheck();
      },
    });
  }

  onSelectExtension(id: string): void {
    this.loading = true;
    this.cdr.markForCheck();
    this.extensionService.getExtension(id).subscribe({
      next: (detail) => {
        this.selectedExtension = detail;
        this.viewMode = 'detail';
        this.loading = false;
        this.cdr.markForCheck();
      },
      error: () => {
        this.snackBar.open('Failed to load extension', 'Close', { duration: 5000 });
        this.loading = false;
        this.cdr.markForCheck();
      },
    });
  }

  onToggleEnabled(event: { id: string; enabled: boolean }): void {
    // Optimistic toggle — update local state then PUT
    const ext = this.extensions.find(e => e.id === event.id);
    if (ext) {
      ext.enabled = event.enabled;
      this.cdr.markForCheck();
    }
  }

  onDeleteExtension(id: string): void {
    this.extensionService.deleteExtension(id).subscribe({
      next: () => {
        this.extensions = this.extensions.filter(e => e.id !== id);
        this.snackBar.open('Extension deleted', 'Close', { duration: 3000 });
        this.cdr.markForCheck();
      },
      error: () => {
        this.snackBar.open('Delete failed', 'Close', { duration: 5000 });
      },
    });
  }

  onSaveExtension(detail: ExtensionDetail): void {
    this.loading = true;
    this.cdr.markForCheck();
    this.extensionService.updateExtension(detail.id, detail as unknown as Record<string, unknown>).subscribe({
      next: (updated) => {
        this.selectedExtension = updated;
        this.snackBar.open('Extension saved', 'Close', { duration: 3000 });
        this.loading = false;
        this.loadExtensions(); // refresh list
      },
      error: () => {
        this.snackBar.open('Save failed', 'Close', { duration: 5000 });
        this.loading = false;
        this.cdr.markForCheck();
      },
    });
  }

  // ---- import flow ----

  onShowImport(): void {
    this.viewMode = 'import';
    this.validationResult = null;
    this.cdr.markForCheck();
  }

  onValidateYaml(yaml: string): void {
    this.extensionService.validateExtension({ _yaml: yaml }).subscribe({
      next: (result) => {
        this.validationResult = result;
        this.cdr.markForCheck();
      },
      error: () => {
        this.snackBar.open('Validation request failed', 'Close', { duration: 5000 });
      },
    });
  }

  onImportExtension(body: Record<string, unknown>): void {
    this.loading = true;
    this.cdr.markForCheck();
    this.extensionService.createExtension(body).subscribe({
      next: () => {
        this.snackBar.open('Extension imported', 'Close', { duration: 3000 });
        this.viewMode = 'list';
        this.loading = false;
        this.loadExtensions();
      },
      error: () => {
        this.snackBar.open('Import failed', 'Close', { duration: 5000 });
        this.loading = false;
        this.cdr.markForCheck();
      },
    });
  }

  // ---- navigation ----

  onBackToList(): void {
    this.viewMode = 'list';
    this.selectedExtension = null;
    this.cdr.markForCheck();
  }
}
