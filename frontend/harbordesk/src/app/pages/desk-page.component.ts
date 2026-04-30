import { AsyncPipe } from '@angular/common';
import { Component, inject } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { BehaviorSubject, combineLatest } from 'rxjs';
import { finalize, switchMap, tap } from 'rxjs/operators';

import { DeliverySurface, ModelEndpointTestResult } from '../core/admin-api.types';
import { HarborDeskAdminApiService } from '../core/admin-api.service';
import { HarborDeskPageId } from '../core/page-registry';
import { PageStatePanelComponent } from '../shared/page-state-panel.component';

@Component({
  standalone: true,
  imports: [AsyncPipe, PageStatePanelComponent],
  template: `
    <hd-page-state-panel
      [state]="state$ | async"
      [savingMemberId]="savingMemberId"
      [saveError]="saveError"
      [saveSuccess]="saveSuccess"
      [testingEndpointId]="testingEndpointId"
      [endpointTestResults]="endpointTestResults"
      [savingTargetId]="savingTargetId"
      [deletingTargetId]="deletingTargetId"
      (defaultDeliverySurfaceChange)="updateDefaultDeliverySurface($event.userId, $event.surface)"
      (notificationTargetDefaultChange)="setDefaultNotificationTarget($event)"
      (notificationTargetDelete)="deleteNotificationTarget($event)"
      (endpointTestRequested)="runEndpointTest($event)"
    ></hd-page-state-panel>
  `
})
export class DeskPageComponent {
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(HarborDeskAdminApiService);
  private readonly refresh$ = new BehaviorSubject(0);

  protected savingMemberId: string | null = null;
  protected saveError: string | null = null;
  protected saveSuccess: string | null = null;
  protected testingEndpointId: string | null = null;
  protected endpointTestResults: Record<string, ModelEndpointTestResult> = {};
  protected savingTargetId: string | null = null;
  protected deletingTargetId: string | null = null;

  protected readonly state$ = combineLatest([this.route.data, this.refresh$]).pipe(
    switchMap(([data]) => this.api.observePage(data['pageId'] as HarborDeskPageId))
  );

  protected updateDefaultDeliverySurface(userId: string, surface: DeliverySurface): void {
    this.savingMemberId = userId;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .updateDefaultDeliverySurface(userId, surface)
      .pipe(
        tap(() => {
          this.saveSuccess = `Default proactive surface saved as ${surface}.`;
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.savingMemberId = null;
        })
      )
      .subscribe({
        error: (error) => {
          this.saveError =
            (error?.error?.error?.message as string | undefined) ??
            (error?.error?.message as string | undefined) ??
            error?.message ??
            'Failed to save the default proactive surface.';
          this.saveSuccess = null;
        }
      });
  }

  protected runEndpointTest(modelEndpointId: string): void {
    this.testingEndpointId = modelEndpointId;
    this.api
      .testModelEndpoint(modelEndpointId)
      .pipe(
        tap((result) => {
          this.endpointTestResults = {
            ...this.endpointTestResults,
            [modelEndpointId]: result
          };
        }),
        finalize(() => {
          this.testingEndpointId = null;
        })
      )
      .subscribe({
        error: (error) => {
          this.endpointTestResults = {
            ...this.endpointTestResults,
            [modelEndpointId]: {
              ok: false,
              status: 'degraded',
              summary:
                (error?.error?.error?.message as string | undefined) ??
                (error?.error?.message as string | undefined) ??
                error?.message ??
                'Endpoint test failed.',
              endpoint: {
                model_endpoint_id: modelEndpointId,
                model_kind: 'unknown',
                endpoint_kind: 'unknown',
                provider_key: 'unknown',
                model_name: 'unknown',
                capability_tags: [],
                cost_policy: {},
                status: 'degraded',
                metadata: {}
              }
            }
          };
        }
      });
  }

  protected setDefaultNotificationTarget(targetId: string): void {
    this.savingTargetId = targetId;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .setDefaultNotificationTarget(targetId)
      .pipe(
        tap(() => {
          this.saveSuccess = 'Default notification target updated.';
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.savingTargetId = null;
        })
      )
      .subscribe({
        error: (error) => {
          this.saveError =
            (error?.error?.error?.message as string | undefined) ??
            (error?.error?.message as string | undefined) ??
            error?.message ??
            'Failed to update the default notification target.';
          this.saveSuccess = null;
        }
      });
  }

  protected deleteNotificationTarget(targetId: string): void {
    this.deletingTargetId = targetId;
    this.saveError = null;
    this.saveSuccess = null;
    this.api
      .deleteNotificationTarget(targetId)
      .pipe(
        tap(() => {
          this.saveSuccess = 'Notification target deleted.';
          this.refresh$.next(Date.now());
        }),
        finalize(() => {
          this.deletingTargetId = null;
        })
      )
      .subscribe({
        error: (error) => {
          this.saveError =
            (error?.error?.error?.message as string | undefined) ??
            (error?.error?.message as string | undefined) ??
            error?.message ??
            'Failed to delete the notification target.';
          this.saveSuccess = null;
        }
      });
  }
}
