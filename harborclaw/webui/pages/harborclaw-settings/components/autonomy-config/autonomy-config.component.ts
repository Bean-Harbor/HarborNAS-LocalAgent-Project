import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { MatCardModule } from '@angular/material/card';
import { MatRadioModule } from '@angular/material/radio';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatIconModule } from '@angular/material/icon';
import { MatChipsModule } from '@angular/material/chips';
import { MatSelectModule } from '@angular/material/select';

import {
  Autonomy,
  AutonomyConfig,
  Channel,
  CHANNEL_META,
} from '../../../interfaces/harborclaw-settings.interface';

@Component({
  selector: 'ix-harborclaw-autonomy-config',
  standalone: true,
  imports: [
    CommonModule,
    FormsModule,
    MatCardModule,
    MatRadioModule,
    MatFormFieldModule,
    MatInputModule,
    MatIconModule,
    MatChipsModule,
    MatSelectModule,
  ],
  templateUrl: './autonomy-config.component.html',
  styleUrls: ['./autonomy-config.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AutonomyConfigComponent {
  @Input() config: AutonomyConfig = {
    default_level: Autonomy.Supervised,
    approval_timeout_seconds: 120,
    allow_full_for_channels: [],
  };
  @Output() configChange = new EventEmitter<AutonomyConfig>();

  readonly levels = [
    {
      value: Autonomy.ReadOnly,
      label: 'Read-Only',
      icon: 'visibility',
      desc: 'Observe only — no mutations allowed.',
    },
    {
      value: Autonomy.Supervised,
      label: 'Supervised',
      icon: 'verified_user',
      desc: 'User confirmation required for risky operations.',
    },
    {
      value: Autonomy.Full,
      label: 'Full Autonomy',
      icon: 'bolt',
      desc: 'Autonomous execution — all operations allowed.',
    },
  ];

  readonly allChannels = CHANNEL_META;

  onLevelChange(level: Autonomy): void {
    this.emit({ default_level: level });
  }

  onTimeoutChange(seconds: number): void {
    this.emit({ approval_timeout_seconds: Math.max(10, seconds) });
  }

  onFullChannelsChange(channels: Channel[]): void {
    this.emit({ allow_full_for_channels: channels });
  }

  private emit(patch: Partial<AutonomyConfig>): void {
    this.configChange.emit({ ...this.config, ...patch });
  }
}
