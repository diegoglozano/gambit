import { test, expect } from "@playwright/test";

test("practice teaches moves visually and keeps preview separate from attempts", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/");
  await page.locator("#username").fill("demo");
  await page.locator("#sync-form button[type=submit]").click();
  await page.locator("#today-review-focus").click();
  await page.locator("#coaching-analyze").click();
  await expect(page.locator("#coaching-board button")).toHaveCount(64);
  await expect(page.locator("#coaching-turn-label")).toHaveText("You play White");
  await expect(page.locator("#coaching-arrow")).toBeHidden();
  await expect(page.locator("#coaching-solution")).toBeHidden();
  await expect(page.locator("#coaching-check")).toBeDisabled();
  await expect(page.locator("#coaching-move")).toBeHidden();

  await page.locator("#coaching-played").click();
  await expect(page.locator("#coaching-board [data-square=h7]")).toHaveAttribute("aria-label", "h7, White queen");
  await expect(page.locator("#coaching-arrow")).toBeVisible();
  await expect(page.locator("#coaching-arrow")).toHaveClass(/played/);
  await expect(page.locator("#coaching-check")).toBeDisabled();
  await expect(page.locator("#coaching-solution")).toBeHidden();
  await page.locator("#coaching-played").click();
  await expect(page.locator("#coaching-board [data-square=g6]")).toHaveAttribute("aria-label", "g6, White queen");

  await page.locator("#coaching-board [data-square=g6]").click();
  await expect(page.locator("#coaching-board [data-square=h6]")).toHaveClass(/legal-target/);
  await page.locator("#coaching-board [data-square=h6]").click();
  await expect(page.locator("#coaching-choice")).toHaveText("Your queen from g6 to h6");
  await expect(page.locator("#coaching-check")).toBeEnabled();
  await page.locator("#coaching-check").click();
  await expect(page.locator("#coaching-feedback")).toContainText("Strong move");
  await expect(page.locator("#game-list")).not.toContainText("NOT ANALYZED");
  await expect(page.locator("#game-list")).not.toContainText("ANALYZING");
  await expect(page.locator("#coaching-feedback")).toContainText("Strong move");
  await expect(page.locator("#coaching-solution")).toBeVisible();
  await expect(page.locator("#coaching-line-next")).toBeVisible();
  await page.locator("#coaching-line-next").click();
  await expect(page.locator("#coaching-board [data-square=g8]")).toHaveAttribute("aria-label", "g8, White queen");
  await expect(page.locator("#coaching-line-status")).toContainText("queen from g6 to g8");
  await expect(page.locator("#coaching-arrow")).toBeVisible();
  await expect(page.locator("#coaching-check")).toBeDisabled();

  // A missing stylesheet can still pass DOM tests. Check the real board and
  // arrow geometry, then verify the layout at desktop and narrow widths.
  for (const width of [1440, 1024, 800, 600]) {
    await page.setViewportSize({ width, height: 1050 });
    const board = await page.locator("#coaching-board").boundingBox();
    const arrow = await page.locator("#coaching-arrow").boundingBox();
    expect(board.width).toBeGreaterThan(250);
    expect(Math.abs(board.width - board.height)).toBeLessThan(1);
    expect(Math.abs(board.width - arrow.width)).toBeLessThan(1);
    expect(Math.abs(board.x - arrow.x)).toBeLessThan(1);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  }
  expect(errors).toEqual([]);
});
