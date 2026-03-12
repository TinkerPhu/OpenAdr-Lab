"""Playwright helpers for driving the VTN UI and VEN UI."""

import os

UI_BASE_URL = os.environ.get("UI_BASE_URL", "http://test-ui:80")
VEN_UI_BASE_URL = os.environ.get("VEN_UI_BASE_URL", "http://test-ven-ui:80")


def tid(testid):
    """Shorthand for data-testid selector."""
    return f'[data-testid="{testid}"]'


class VtnUi:
    """Wraps a Playwright page with VTN UI-specific actions."""

    def __init__(self, page):
        self.page = page

    def open(self):
        self.page.goto(UI_BASE_URL)
        self.page.wait_for_selector(tid("nav-dashboard"))

    # -- navigation --

    def go_programs(self):
        self.page.click(tid("nav-programs"))
        self.page.wait_for_selector(tid("programs-heading"))

    def go_events(self):
        self.page.click(tid("nav-events"))
        self.page.wait_for_selector(tid("events-heading"))

    def go_reports(self):
        self.page.click(tid("nav-reports"))
        self.page.wait_for_selector(tid("reports-heading"))

    # -- programs --

    def create_program(self, name, ven_targets=None, description_url=None,
                       long_name=None, program_type=None):
        self.page.click(tid("create-program-btn"))
        self.page.wait_for_selector(tid("program-form-dialog"))
        self.page.fill(tid("program-name-input"), name)
        if long_name:
            self.page.fill(tid("program-long-name-input"), long_name)
        if program_type:
            self.page.fill(tid("program-type-input"), program_type)
        if description_url:
            self.page.fill(tid("program-description-url-input"), description_url)
        if ven_targets:
            for ven in ven_targets:
                self.page.click(tid(f"ven-checkbox-{ven}"))
        self.page.click(tid("program-form-submit"))
        self.page.wait_for_selector(tid("program-form-dialog"), state="detached")

    def program_visible(self, name, timeout=10000):
        """Check if a program with the given name is visible in the list."""
        try:
            self.page.wait_for_selector(
                f'{tid("programs-list")} >> text="{name}"', timeout=timeout
            )
            return True
        except Exception:
            return False

    def delete_program(self, program_id):
        self.page.click(tid(f"delete-program-{program_id}"))
        self.page.wait_for_selector(tid("confirm-dialog"))
        self.page.click(tid("confirm-dialog-ok"))
        self.page.wait_for_selector(tid("confirm-dialog"), state="detached")

    # -- events --

    def create_event(self, name, program_name, priority=None,
                     start=None, duration=None, intervals_json="[]",
                     targets_json=None):
        self.page.click(tid("create-event-btn"))
        self.page.wait_for_selector(tid("event-form-dialog"))
        self.page.fill(tid("event-name-input"), name)

        # MUI Select: click the wrapping div (parent of the hidden input)
        # to open the dropdown, then click the menu item by text.
        select_input = self.page.locator(tid("event-program-select"))
        select_input.locator("..").click()
        self.page.locator(f'li[role="option"]:has-text("{program_name}")').click()

        if priority is not None:
            self.page.fill(tid("event-priority-input"), str(priority))
        if start:
            self.page.fill(tid("event-start-input"), start)
        if duration:
            self.page.fill(tid("event-duration-input"), duration)
        if intervals_json != "[]":
            # Clear default "[]" then type new value
            self.page.fill(tid("event-intervals-input"), "")
            self.page.fill(tid("event-intervals-input"), intervals_json)
        if targets_json:
            self.page.fill(tid("event-targets-input"), targets_json)
        self.page.click(tid("event-form-submit"))
        self.page.wait_for_selector(tid("event-form-dialog"), state="detached")

    def event_visible(self, name, timeout=10000):
        """Check if an event row with the given name is visible."""
        try:
            self.page.wait_for_selector(
                f'{tid("events-table")} >> text="{name}"', timeout=timeout
            )
            return True
        except Exception:
            return False

    def delete_event(self, event_id):
        self.page.click(tid(f"delete-event-{event_id}"))
        self.page.wait_for_selector(tid("confirm-dialog"))
        self.page.click(tid("confirm-dialog-ok"))
        self.page.wait_for_selector(tid("confirm-dialog"), state="detached")

    def delete_event_by_name(self, name):
        """Delete an event by clicking its delete button (found via aria-label)."""
        self.page.click(f'button[aria-label="Delete {name}"]')
        self.page.wait_for_selector(tid("confirm-dialog"))
        self.page.click(tid("confirm-dialog-ok"))
        self.page.wait_for_selector(tid("confirm-dialog"), state="detached")

    def event_not_visible(self, name, timeout=10000):
        """Wait until an event row with the given name is gone from the table."""
        try:
            self.page.wait_for_selector(
                f'{tid("events-table")} >> text="{name}"',
                state="detached", timeout=timeout
            )
            return True
        except Exception:
            return False

    # -- reports --

    def report_visible(self, client_name, timeout=10000):
        """Check if a report from client_name is visible.

        Retries once with a page reload if the first attempt fails,
        since the reports page loads data once on navigation.
        """
        selector = f'{tid("reports-table")} >> text="{client_name}"'
        try:
            self.page.wait_for_selector(selector, timeout=timeout)
            return True
        except Exception:
            pass
        # Reload and retry once
        self.page.reload()
        self.page.wait_for_selector(tid("reports-heading"))
        try:
            self.page.wait_for_selector(selector, timeout=timeout)
            return True
        except Exception:
            return False


class VenUi:
    """Wraps a Playwright page with VEN UI-specific actions."""

    def __init__(self, page):
        self.page = page

    def open(self):
        self.page.goto(VEN_UI_BASE_URL)
        self.page.wait_for_selector(tid("nav-simulation"))

    def go_simulation(self):
        self.page.click(tid("nav-simulation"))
        self.page.wait_for_selector(tid("ev-charge-caption"), timeout=15000)

    def go_controller(self):
        self.page.click(tid("nav-controller"))
        self.page.wait_for_selector(tid("controller-packets-table"), timeout=15000)
