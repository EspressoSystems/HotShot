import re
import time
from dataclasses import dataclass

missing_deps = False
try:
    import requests
except ImportError:
    print("please run `pip install requests`")
    missing_deps = True

try:
    import rich
    from rich.live import Live
    from rich.table import Table
except ImportError:
    print("please run `pip install rich`")
    missing_deps = True

try:
    import typer
except ImportError:
    print("please run `pip install typer`")
    missing_deps = True

if missing_deps:
    print('please install missing packages before continuing')
    exit(1)

app = typer.Typer()

REPOS = [
    "HotShot",
    "hotshot-query-service",
    "hotshot-events-service",
    "hotshot-builder-core",
    "espresso-sequencer",
]

PORT_RANGE = list(range(24000, 24005))


@dataclass
class ReplicaData:
    last_highest_view = 0
    last_decided_view = 0
    number_of_empty_blocks_proposed = 0
    last_synced_block_height = 0
    highest_view_updates = 0
    decided_view_updates = 0
    synced_block_height_updates = 0
    status = True


class AreWeWorking:
    def __init__(self):
        self.last_highest_view = 0
        self.highest_view_updates = 0
        self.decided_view_updates = 0
        self.last_decided_view = 0
        self.number_of_empty_blocks_proposed = 0
        self.synced_block_height_updates = 0
        self.tolerance = 5
        self.replica_data = {}
        self.failed_views = set()

    def table(self):
        table = Table(title="Port Status")
        table.add_column("Port", justify="right", no_wrap=True)
        table.add_column("Last Highest View")
        table.add_column("Last Decided View")
        table.add_column("Num Empty Blocks")
        table.add_column("Last Synced Block Height")
        table.add_column("Status")

        for port, data in self.replica_data.items():
            table.add_row(
                str(port),
                str(data.last_highest_view),
                str(data.last_decided_view),
                str(data.number_of_empty_blocks_proposed),
                str(data.last_synced_block_height),
                "[green]In Sync[/green]"
                if data.status
                else "[bold red]Out of Sync[/bold red]",
            )

        return table

    def update(self, text: str, port: str):
        if port not in self.replica_data:
            self.replica_data[port] = ReplicaData()
        self.update_last_decided_view(text, port)
        self.update_number_of_empty_blocks_proposed(text, port)
        self.update_last_highest_view(text, port)
        self.update_last_synced_block_height(text, port)

    def update_last_highest_view(self, text: str, port: str):
        match = re.search(r"^consensus_current_view (\d+)", text, re.MULTILINE)
        if match:
            current_view = int(match.group(1))
            if current_view > self.replica_data[port].last_highest_view:
                self.replica_data[port].last_highest_view = current_view
                self.replica_data[port].highest_view_updates = 0
            else:
                self.replica_data[port].highest_view_updates += 1
            if self.replica_data[port].highest_view_updates > self.tolerance:
                self.replica_data[port].status = False

    def update_last_decided_view(self, text: str, port: str):
        match = re.search(r"^consensus_last_decided_view (\d+)", text, re.MULTILINE)
        if match:
            last_decided_view = int(match.group(1))
            if last_decided_view > self.replica_data[port].last_decided_view:
                self.replica_data[port].last_decided_view = last_decided_view
                self.replica_data[port].decided_view_updates = 0
                self.last_decided_view = max(
                    self.last_decided_view, self.replica_data[port].last_decided_view
                )
            else:
                self.replica_data[port].decided_view_updates += 1
            if self.replica_data[port].decided_view_updates > self.tolerance:
                self.replica_data[port].status = False
                self.failed_views.add(last_decided_view)

    def update_number_of_empty_blocks_proposed(self, text: str, port: str):
        match = re.search(r"^consensus_number_of_empty_blocks_proposed (\d+)", text, re.MULTILINE)
        if match:
            number_of_empty_blocks_proposed = int(match.group(1))
            if number_of_empty_blocks_proposed > self.replica_data[port].number_of_empty_blocks_proposed:
                self.replica_data[port].number_of_empty_blocks_proposed = number_of_empty_blocks_proposed
                self.number_of_empty_blocks_proposed = max(
                    self.number_of_empty_blocks_proposed, self.replica_data[port].number_of_empty_blocks_proposed
                )

    def update_last_synced_block_height(self, text: str, port: str):
        match = re.search(
            r"^consensus_last_synced_block_height (\d+)", text, re.MULTILINE
        )
        if match:
            last_synced_block_height = int(match.group(1))
            if (
                last_synced_block_height
                > self.replica_data[port].last_synced_block_height
            ):
                self.replica_data[
                    port
                ].last_synced_block_height = last_synced_block_height
                self.replica_data[port].synced_block_height_updates = 0
            else:
                self.replica_data[port].synced_block_height_updates += 1
            if self.replica_data[port].synced_block_height_updates > self.tolerance:
                self.replica_data[port].status = False


@app.command(help="Watches the native demo and reports any HotShot failures.")
def evaluate(view_threshold: int=10_000, host_ip="localhost"):
    working = AreWeWorking()
    with Live(working.table(), refresh_per_second=1) as live:
        while working.last_decided_view < int(view_threshold):
            for port in PORT_RANGE:
                res = requests.get(
                    f"http://{host_ip}:{port}/status/metrics", allow_redirects=True
                )
                if res.status_code == 200:
                    working.update(res.text, port)
                else:
                    rich.print(
                        f"Request to: http://{host_ip}:{port}/status/metrics failed"
                    )
            live.update(working.table())
            time.sleep(1)
    if len(working.failed_views) == 0:
        rich.print("[green]Daily build successful![/green]")


if __name__ == "__main__":
    app()
