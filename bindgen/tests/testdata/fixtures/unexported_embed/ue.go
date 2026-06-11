package unexported_embed

// conn is unexported; its exported methods promote onto the exported embedders.
type conn struct{ fd int }

func (c *conn) Read() int  { return c.fd }
func (c *conn) Close() int { return 0 }

// IPConn and TCPConn both embed the same unexported conn.
type IPConn struct{ conn }
type TCPConn struct{ conn }

// Plain has its own method plus the embed, to check mixed promotion.
type Plain struct {
	conn
	Extra int
}

func (Plain) Local() int { return 1 }
