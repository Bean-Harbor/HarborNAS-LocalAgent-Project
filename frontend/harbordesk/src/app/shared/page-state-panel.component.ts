import { NgClass } from '@angular/common';
import { Component, EventEmitter, Input, Output } from '@angular/core';

import {
  DeliverySurface,
  DeskPageModel,
  FeatureAvailabilityStatus,
  MetricTone,
  ModelEndpointTestResult,
  PageState,
  SetupStepState
} from '../core/admin-api.types';

@Component({
  selector: 'hd-page-state-panel',
  standalone: true,
  imports: [NgClass],
  templateUrl: './page-state-panel.component.html',
  styleUrl: './page-state-panel.component.css'
})
export class PageStatePanelComponent {
  @Input() state: PageState<DeskPageModel> | null = null;
  @Input() savingMemberId: string | null = null;
  @Input() saveError: string | null = null;
  @Input() saveSuccess: string | null = null;
  @Input() testingEndpointId: string | null = null;
  @Input() endpointTestResults: Record<string, ModelEndpointTestResult> = {};
  @Input() savingTargetId: string | null = null;
  @Input() deletingTargetId: string | null = null;

  @Output() readonly defaultDeliverySurfaceChange = new EventEmitter<{
    userId: string;
    surface: DeliverySurface;
  }>();
  @Output() readonly notificationTargetDefaultChange = new EventEmitter<string>();
  @Output() readonly notificationTargetDelete = new EventEmitter<string>();
  @Output() readonly endpointTestRequested = new EventEmitter<string>();

  protected toneClass(tone: MetricTone): string {
    return `tone-${tone}`;
  }

  protected setupToneClass(state: SetupStepState): string {
    switch (state) {
      case 'ready':
        return 'tone-good';
      case 'blocked':
        return 'tone-danger';
      case 'needs-config':
      case 'read-only':
      default:
        return 'tone-warn';
    }
  }

  protected requestDefaultSurfaceChange(userId: string, surface: string): void {
    if (surface !== 'feishu' && surface !== 'weixin') {
      return;
    }
    this.defaultDeliverySurfaceChange.emit({
      userId,
      surface
    });
  }

  protected requestEndpointTest(modelEndpointId: string): void {
    this.endpointTestRequested.emit(modelEndpointId);
  }

  protected requestNotificationTargetDefaultChange(targetId: string): void {
    this.notificationTargetDefaultChange.emit(targetId);
  }

  protected requestNotificationTargetDelete(targetId: string): void {
    this.notificationTargetDelete.emit(targetId);
  }

  protected featureStatusToneClass(status: FeatureAvailabilityStatus): string {
    switch (status) {
      case 'available':
        return 'tone-good';
      case 'blocked':
        return 'tone-danger';
      case 'degraded':
      case 'not_configured':
      default:
        return 'tone-warn';
    }
  }

  protected featureFallbackLabel(values: string[]): string {
    return values.length > 0 ? values.join(' -> ') : 'none';
  }

  protected featureWhereToEdit(featureId: string): string {
    switch (featureId) {
      case 'retrieval.ocr':
      case 'retrieval.embed':
      case 'retrieval.answer':
      case 'retrieval.vision_summary':
        return 'Models & Policies';
      case 'interactive_reply':
      case 'proactive_delivery':
        return 'System Settings';
      case 'binding_availability':
        return 'Account Management';
      default:
        return 'Admin API projection';
    }
  }
}
