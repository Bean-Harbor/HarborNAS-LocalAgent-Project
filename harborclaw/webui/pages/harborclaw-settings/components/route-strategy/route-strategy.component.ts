import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { CdkDragDrop, DragDropModule, moveItemInArray } from '@angular/cdk/drag-drop';
import { MatCardModule } from '@angular/material/card';
import { MatIconModule } from '@angular/material/icon';
import { MatChipsModule } from '@angular/material/chips';

import {
  Route,
  RouteStatus,
  DEFAULT_ROUTE_PRIORITY,
} from '../../../interfaces/harborclaw-settings.interface';

@Component({
  selector: 'ix-harborclaw-route-strategy',
  standalone: true,
  imports: [
    CommonModule,
    DragDropModule,
    MatCardModule,
    MatIconModule,
    MatChipsModule,
  ],
  templateUrl: './route-strategy.component.html',
  styleUrls: ['./route-strategy.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class RouteStrategyComponent {
  @Input() routePriority: Route[] = [...DEFAULT_ROUTE_PRIORITY];
  @Input() routeStatuses: RouteStatus[] = [];
  @Output() routePriorityChange = new EventEmitter<Route[]>();

  readonly routeLabels: Record<Route, string> = {
    [Route.MiddlewareApi]: 'Middleware API',
    [Route.Midcli]: 'midcli (CLI)',
    [Route.Browser]: 'Browser Automation',
    [Route.Mcp]: 'MCP Protocol',
  };

  readonly routeIcons: Record<Route, string> = {
    [Route.MiddlewareApi]: 'api',
    [Route.Midcli]: 'terminal',
    [Route.Browser]: 'language',
    [Route.Mcp]: 'hub',
  };

  isAvailable(route: Route): boolean {
    return this.routeStatuses.find((s) => s.route === route)?.available ?? false;
  }

  onDrop(event: CdkDragDrop<Route[]>): void {
    const list = [...this.routePriority];
    moveItemInArray(list, event.previousIndex, event.currentIndex);
    this.routePriorityChange.emit(list);
  }
}
