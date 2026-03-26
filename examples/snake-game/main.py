import random
import sys

import pygame


CELL = 20
GRID_W = 30
GRID_H = 20
WIDTH = GRID_W * CELL
HEIGHT = GRID_H * CELL
FPS = 12

BLACK = (20, 20, 20)
GREEN = (60, 180, 75)
RED = (230, 80, 80)
WHITE = (245, 245, 245)


def random_food(snake):
    while True:
        p = (random.randint(0, GRID_W - 1), random.randint(0, GRID_H - 1))
        if p not in snake:
            return p


def draw_cell(surface, pos, color):
    x, y = pos
    rect = pygame.Rect(x * CELL, y * CELL, CELL - 1, CELL - 1)
    pygame.draw.rect(surface, color, rect)


def run():
    pygame.init()
    screen = pygame.display.set_mode((WIDTH, HEIGHT))
    pygame.display.set_caption("Snake")
    clock = pygame.time.Clock()
    font = pygame.font.SysFont(None, 28)

    snake = [(GRID_W // 2, GRID_H // 2)]
    direction = (1, 0)
    pending_dir = direction
    food = random_food(snake)
    score = 0
    game_over = False

    while True:
        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                pygame.quit()
                sys.exit(0)
            if event.type == pygame.KEYDOWN:
                if event.key == pygame.K_q:
                    pygame.quit()
                    sys.exit(0)
                if game_over and event.key == pygame.K_r:
                    snake = [(GRID_W // 2, GRID_H // 2)]
                    direction = (1, 0)
                    pending_dir = direction
                    food = random_food(snake)
                    score = 0
                    game_over = False
                if event.key == pygame.K_UP and direction != (0, 1):
                    pending_dir = (0, -1)
                elif event.key == pygame.K_DOWN and direction != (0, -1):
                    pending_dir = (0, 1)
                elif event.key == pygame.K_LEFT and direction != (1, 0):
                    pending_dir = (-1, 0)
                elif event.key == pygame.K_RIGHT and direction != (-1, 0):
                    pending_dir = (1, 0)

        if not game_over:
            direction = pending_dir
            hx, hy = snake[0]
            nx, ny = hx + direction[0], hy + direction[1]

            # Wall or self collision ends game.
            if nx < 0 or nx >= GRID_W or ny < 0 or ny >= GRID_H or (nx, ny) in snake:
                game_over = True
            else:
                snake.insert(0, (nx, ny))
                if (nx, ny) == food:
                    score += 1
                    food = random_food(snake)
                else:
                    snake.pop()

        screen.fill(BLACK)
        for s in snake:
            draw_cell(screen, s, GREEN)
        draw_cell(screen, food, RED)

        score_text = font.render(f"Score: {score}", True, WHITE)
        screen.blit(score_text, (8, 8))

        if game_over:
            msg = font.render("Game Over - Press R to restart, Q to quit", True, WHITE)
            rect = msg.get_rect(center=(WIDTH // 2, HEIGHT // 2))
            screen.blit(msg, rect)

        pygame.display.flip()
        clock.tick(FPS)


if __name__ == "__main__":
    run()
