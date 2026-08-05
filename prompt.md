# Build Prompt: Kelyfos — SSH Workspace Manager

## Role

You are a senior desktop application architect and engineer.

Build a production-quality Windows desktop application called **Kelyfos**.

The application is a modern local-first SSH workspace manager inspired by Terminus, VS Code terminal workflows, and developer productivity tools.

The goal is not to build a generic terminal emulator.

The goal is to build a **workspace operating system for developers**:

* Saved SSH connections
* Persistent workspaces
* Custom terminal layouts
* Startup automation
* Local encrypted credentials
* Future AI agent integration

---

# Product Principles

The application must be:

* Windows native feeling
* Local only
* No cloud
* No accounts
* No telemetry
* No analytics
* Fast startup
* Keyboard friendly
* Modern dark UI
* Modular architecture

Everything belongs to the user's local machine.

---

# Required Technology

Use:

Frontend:

* React
* TypeScript
* Zustand
* Modern component library

Desktop:

* Tauri 2

Backend:

* Rust

Terminal:

* xterm.js

Database:

* SQLite

Secrets:

* Windows Credential Manager

Layout:

* FlexLayout, GoldenLayout, or equivalent maintained docking library

Do not use Electron.

---

# Core Architecture

The application has four main concepts:

```
SSH Connections
        |
        |
        v

Workspaces
        |
        |
        v

Terminal Panes
        |
        |
        v

Startup Tasks + Variables
```

---

# SSH Connections

Connections are reusable.

They describe HOW to connect.

They do not contain workspace behavior.

Example:

```
Production Server
Database Server
Redis Server
Staging Server
```

Each connection stores:

## Basic

* Name
* Host
* Port
* Username
* Description
* Icon
* Color
* Tags

## Authentication

Support:

### Password

Store password only using:

Windows Credential Manager

SQLite stores:

* Credential ID only

Never:

* plaintext passwords
* encrypted passwords inside SQLite

### SSH Key

Store:

* Private key path
* Optional passphrase reference

Passphrase stored using:

Windows Credential Manager

---

# Workspaces

A workspace describes a working environment.

Example:

```
Customer A Production

    Terminal 1
        Production SSH

    Terminal 2
        Database SSH

    Terminal 3
        Logs SSH
```

A workspace contains:

* Name
* Description
* Icon
* Variables
* Layout
* Terminal definitions
* Startup tasks
* Last active state

Multiple workspaces can be open simultaneously.

---

# Workspace Persistence

The application must remember:

* Open workspaces
* Active workspace
* Terminal positions
* Layout
* Selected pane
* Connection state where possible

When the application restarts:

Restore previous state automatically.

---

# Terminal Panes

Each terminal pane references:

* One SSH connection
* One workspace
* One startup task list

Each terminal has:

* Custom title
* Custom variables
* Custom startup tasks
* Position in layout

---

# Terminal UI

Use xterm.js.

Support:

* ANSI colors
* Unicode
* Mouse interaction
* Copy
* Paste
* Scrollback
* Resize
* Reconnect
* Search

Do not implement a terminal emulator.

---

# Custom Grid Layout

The terminal area must support:

* Drag and drop panes
* Resize panes
* Split horizontally
* Split vertically
* Nested layouts
* Tabs inside layouts

Behavior should resemble:

* VS Code editor layout
* Terminus terminal layout

The layout must serialize and restore.

---

# Workspace Variables

Implement a variable system.

Variables can exist at three levels:

## Global

Example:

```
EDITOR=nvim
PROJECT_ROOT=/srv/projects
```

## Workspace

Example:

```
CLIENT=Acme
PROJECT_DIR=/srv/acme
ENV=production
```

## Terminal

Example:

```
SERVICE=frontend
```

Priority:

```
Terminal
   >
Workspace
   >
Global
```

---

# Variable Usage

Variables are available in:

* Startup commands
* Terminal titles
* Future AI context

Example:

Command:

```
cd ${PROJECT_DIR}
```

Becomes:

```
cd /srv/acme
```

---

# Startup Task Engine

Do not implement simple startup command strings.

Implement a small task engine.

Example:

```yaml
tasks:

- type: wait_for_prompt

- type: command
  value: cd ${PROJECT_DIR}

- type: wait
  milliseconds: 2000

- type: command
  value: docker compose logs
```

Initial supported tasks:

* wait_for_prompt
* command
* wait

Design so future tasks can be added:

* confirmation
* conditional execution
* open terminal
* notification
* AI generated tasks

---

# Sidebar

Left navigation:

## Workspaces

Features:

* Create
* Rename
* Delete
* Duplicate
* Search
* Import
* Export

## Connections

Features:

* Create
* Edit
* Delete
* Duplicate
* Search
* Tags
* Folders

---

# Workspace Tabs

Top-level workspace tabs.

Each tab shows status.

Statuses:

* Connected
* Connecting
* Disconnected
* Error
* AI Running
* Waiting Approval

Use icons/badges.

Design status system as extensible.

---

# Themes

Dark themes only.

Include:

* Several built-in themes

Allow customization:

Terminal:

* Font family
* Font size
* Line height
* Cursor
* Cursor blink
* Foreground color
* Background color
* ANSI colors
* Padding

Application:

* UI density
* Accent colors

---

# Import / Export

Support exporting:

* Connections
* Workspaces
* Variables
* Layouts
* Settings

Format:

JSON

Passwords:

NEVER export.

When importing:

Prompt user for missing credentials.

The export format must include version information.

Example:

```
{
 "version":1,
 "workspace":{}
}
```

---

# Workspace Files

Although SQLite is the main database, every workspace must be representable as a standalone JSON file.

Reason:

* Backup
* Sharing
* Git storage
* Migration

Example:

```
customer-a.workspace.json
```

The application should be able to:

* Import workspace files
* Export workspace files
* Recreate the workspace from the file

---

# Storage

SQLite stores:

* Connections
* Workspaces
* Layouts
* Variables
* Startup tasks
* Settings

Windows Credential Manager stores:

* Passwords
* SSH key passphrases

---

# Future AI Architecture

Do not implement AI now.

Prepare the architecture.

Future:

Each workspace can have:

* Multiple AI agents

Agents have:

* State
* Conversation history
* Permissions
* Context

Workspace tabs will display:

* Agent running
* Agent waiting approval
* Agent error

---

# Future MCP Server

Design for future MCP support.

The application may expose:

Workspace tools:

* List workspaces
* Read workspace variables
* Read terminal output
* Open terminal
* Execute command
* Read files
* Write files

Permissions are workspace-specific.

Example:

```
Production Workspace

Allowed:
read terminal

Denied:
execute commands
```

---

# Security Requirements

Never:

* Store passwords in SQLite
* Log credentials
* Export credentials
* Send data externally

All communication stays local.

---

# Project Structure

Create clear separation:

```
src/

frontend/

backend/

modules/

    terminal
    ssh
    workspace
    layout
    variables
    startup
    storage
    settings
    themes
    ai
    mcp
```

---

# Development Order

## Phase 1 MVP

Build:

1. Tauri application
2. React UI
3. SQLite storage
4. SSH connections
5. Credential Manager integration
6. xterm.js terminal
7. Workspace management
8. Multi-pane layout
9. Variable system
10. Startup task engine
11. Import/export
12. Themes
13. State restoration

## Phase 2

Prepare and implement:

* AI agents
* AI chat panel
* Image paste
* Workspace context
* MCP server
* Permission system

---

# Required Deliverables

Before coding:

Provide:

1. Architecture diagram
2. Database schema
3. Folder structure
4. Rust modules
5. React component tree
6. IPC API design
7. State management design
8. Workspace JSON schema
9. Implementation roadmap

Then implement incrementally.

Do not create unnecessary complexity.

Prioritize:

1. Correct architecture
2. Security
3. Maintainability
4. User experience
5. Feature completeness
