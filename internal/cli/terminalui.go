package cli

import (
	"context"
	"encoding/json"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/execution"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/tasks"
	"github.com/GrayCodeAI/rover/internal/terminal"
	"os"
	"strings"
	"time"
)

func (a *extendedApp) tui(parent context.Context, s *store.Store) int {
	for {
		code, socket := a.tuiScreen(parent, s)
		if socket == "" {
			return code
		}
		if e := execution.Attach(parent, socket, os.Stdin, os.Stdout); e != nil {
			fmt.Fprintln(a.Err, execution.SafeText(e.Error()))
		}
	}
}

// tui is a keyboard client of the same task/evidence services. It is not a
// terminal emulator or a separate authority for acceptance.
func (a *extendedApp) tuiScreen(parent context.Context, s *store.Store) (int, string) {
	if !terminal.IsTTY(os.Stdin) || !terminal.IsTTY(os.Stdout) {
		fmt.Fprintln(a.Err, "Interactive TUI requires a Linux terminal; use status --json or ui --watch for pipes.")
		return 2, ""
	}
	ctx, cancel := context.WithCancel(parent)
	defer cancel()
	restore, e := terminal.Raw(os.Stdin)
	if e != nil {
		return a.fail(e), ""
	}
	defer func() { restore(); fmt.Fprint(a.Out, "\x1b[?25h\x1b[?1049l") }()
	fmt.Fprint(a.Out, "\x1b[?1049h\x1b[?25l")
	keys := make(chan []byte, 8)
	readerDone := make(chan struct{})
	go func() {
		defer close(readerDone)
		buf := make([]byte, 4096)
		for {
			n, e := terminal.Read(ctx, os.Stdin, buf)
			if e != nil {
				return
			}
			select {
			case keys <- append([]byte(nil), buf[:n]...):
			case <-ctx.Done():
				return
			}
		}
	}()
	defer func() { cancel(); <-readerDone }()
	tick := time.NewTicker(600 * time.Millisecond)
	defer tick.Stop()
	index, scroll := 0, 0
	view, message := "tasks", ""
	var rows []model.TaskRun
	var confirm *model.TaskRun
	action := ""
	render := func() {
		_ = tasks.Reconcile(s)
		raw, e := s.List("task", 200)
		if e != nil {
			message = e.Error()
		}
		rows = nil
		for _, b := range raw {
			var r model.TaskRun
			if json.Unmarshal(b, &r) == nil {
				rows = append(rows, r)
			}
		}
		if index >= len(rows) {
			index = len(rows) - 1
		}
		if index < 0 {
			index = 0
		}
		dim := terminal.Size(os.Stdout)
		width, height := int(dim.Cols), int(dim.Rows)
		if width > 180 {
			width = 180
		}
		if width < 30 {
			width = 30
		}
		if height < 10 {
			height = 10
		}
		lines := []string{"ROVER  " + model.Version + "  |  LOCAL ADVISORY", "tasks [t]  logs [l]  evidence [e]  diff [d]  attach [a]", "j/k move  review [v]  cancel [c]  refresh [r]  quit [q]", strings.Repeat("─", width)}
		if confirm != nil {
			lines = append(lines, "CONFIRM "+strings.ToUpper(action)+" for "+confirm.ID, "Candidate: "+confirm.Candidate, "[y] confirm    [n] abort; local review is not authenticated team approval")
		} else if view == "tasks" {
			if len(rows) == 0 {
				lines = append(lines, "No tasks. Start one with rover task run --file task.json --allow-local")
			}
			for i, r := range rows {
				prefix := "  "
				if i == index {
					prefix = "> "
				}
				lines = append(lines, fmt.Sprintf("%s%-30s %-18s %s", prefix, r.ID, r.Status, r.Contract.Objective))
			}
		} else if len(rows) > 0 {
			r := rows[index]
			lines = append(lines, r.ID+"  "+r.Status)
			switch view {
			case "logs":
				h := ""
				if r.Process != nil {
					h = r.Process.StdoutSHA256
				}
				if h != "" {
					b, e := s.ReadBlob(h)
					if e == nil {
						lines = append(lines, strings.Split(string(b), "\n")...)
					}
				} else {
					lines = append(lines, "Process is active; use logs --follow for streamed file output.")
				}
			case "evidence":
				if r.InvestigationID != "" {
					var in model.Investigation
					if s.Get("investigation", r.InvestigationID, &in) == nil {
						lines = append(lines, "Candidate: "+in.Candidate, "Policy decision: "+in.Decision)
						for _, c := range in.Checks {
							lines = append(lines, fmt.Sprintf("%-24s %-14s %s", c.ID, c.Outcome, c.Meaning))
						}
						for _, u := range in.Unknowns {
							lines = append(lines, "UNKNOWN: "+u)
						}
						for _, f := range in.Findings {
							lines = append(lines, "FINDING: "+f.Path+" "+f.Message)
						}
					}
				} else {
					lines = append(lines, "No investigation yet; execution completion is not acceptance.")
				}
			case "diff":
				if r.Candidate != "" {
					base, e1 := source.Load(s, r.BaseSnapshot)
					cand, e2 := source.Load(s, r.Candidate)
					if e1 == nil && e2 == nil {
						for _, c := range source.Compare(base, cand).Changes {
							lines = append(lines, fmt.Sprintf("%-10s %-16s %s", c.Status, c.Category, c.Path))
						}
						lines = append(lines, "Use rover diff --base <snapshot> --candidate <snapshot> for the complete applicable patch.")
					}
				}
			}
		}
		lines = append(lines, "", message)
		fmt.Fprint(a.Out, "\x1b[H\x1b[2J")
		header := 4
		for i, line := range lines {
			if i >= header && i < header+scroll {
				continue
			}
			line = strings.ReplaceAll(strings.ReplaceAll(execution.SafeText(line), "\r", " "), "\n", " ")
			run := []rune(line)
			if len(run) > width {
				line = string(run[:width-1]) + "…"
			}
			fmt.Fprint(a.Out, line, "\r\n")
			if i-scroll >= height-2 {
				break
			}
		}
	}
	render()
	for {
		select {
		case <-ctx.Done():
			return 0, ""
		case <-tick.C:
			render()
		case b := <-keys:
			key := byte(0)
			if len(b) > 0 {
				key = b[0]
			}
			if string(b) == "\x1b[A" {
				key = 'k'
			}
			if string(b) == "\x1b[B" {
				key = 'j'
			}
			if confirm != nil {
				if key == 'y' {
					current, e := tasks.Get(s, confirm.ID)
					if e != nil {
						message = e.Error()
					} else if current.Candidate != confirm.Candidate || current.InvestigationID != confirm.InvestigationID {
						message = "Candidate changed: action aborted."
					} else if action == "cancel" {
						e = tasks.Cancel(s, current.ID)
						message = "Cancellation requested; prior external actions are not undone."
						if e != nil {
							message = e.Error()
						}
					} else {
						v, e := assurance.Review(s, current.InvestigationID, "Reviewed exact displayed candidate in local TUI")
						if e != nil {
							message = e.Error()
						} else {
							message = "Local review recorded: " + v.ID
						}
					}
					confirm = nil
				} else if key == 'n' || key == 27 {
					confirm = nil
				}
				render()
				continue
			}
			switch key {
			case 'q', 3:
				return 0, ""
			case 'j':
				if view == "tasks" {
					if index+1 < len(rows) {
						index++
					}
				} else {
					scroll++
				}
			case 'k':
				if view == "tasks" {
					if index > 0 {
						index--
					}
				} else if scroll > 0 {
					scroll--
				}
			case 't':
				view = "tasks"
				scroll = 0
			case 'l':
				view = "logs"
				scroll = 0
			case 'e':
				view = "evidence"
				scroll = 0
			case 'd':
				view = "diff"
				scroll = 0
			case 'v', 'c':
				if len(rows) > 0 {
					x := rows[index]
					confirm = &x
					action = "review"
					if key == 'c' {
						action = "cancel"
					}
				}
			case 'a':
				if len(rows) > 0 {
					r := rows[index]
					if !r.Contract.Interactive {
						message = "This task is headless; no interactive terminal exists."
						break
					}
					return 0, r.Socket
				}
			case 'r':
				message = "Refreshed."
			}
			render()
		}
	}
}
