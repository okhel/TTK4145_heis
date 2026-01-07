// Use `go run foo.go` to run your program

package main

import (
	. "fmt"
	"runtime"
	"time"
)

var i = 0
var ch = make(chan int)
var done = make(chan bool)

func incrementing(ch chan int, done chan bool) {
	//TODO: increment i 1000000 times

	for j := 0; j <= 1000000; j++ {
		ch <- 1
	}

	done <- true
}

func decrementing(ch chan int, done chan bool) {
	//TODO: decrement i 1000000 times
	for k := 0; k <= 1000000; k++ {
		ch <- -1
	}

	done <- true
}

func main() {
	// What does GOMAXPROCS do? What happens if you set it to 1?
	runtime.GOMAXPROCS(2)

	// TODO: Spawn both functions as goroutines
	doneCount := 0

	go incrementing(ch, done)
	go decrementing(ch, done)

	for doneCount < 2 {
		select {
		case op := <-ch:
			i = i + op

		case <-done:
			doneCount++
		}
	}

	// We have no direct way to wait for the completion of a goroutine (without additional synchronization of some sort)
	// We will do it properly with channels soon. For now: Sleep.
	time.Sleep(500 * time.Millisecond)
	Println("The magic number is:", i)
}
