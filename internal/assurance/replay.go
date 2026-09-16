package assurance

func ExitCode(d string) int {
	switch d {
	case "ACCEPTED":
		return 0
	case "BLOCKED":
		return 1
	case "REVIEW_REQUIRED":
		return 3
	default:
		return 2
	}
}
