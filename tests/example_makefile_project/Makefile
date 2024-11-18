CXX=g++

CXXFLAGS=-std=c++11

SRC_PATH=src

all: main

main: $(SRC_PATH)/main.cpp
	$(CXX) $(CXXFLAGS) $(CPPFLAGS) -o $@ $^

clean:
	rm -f main