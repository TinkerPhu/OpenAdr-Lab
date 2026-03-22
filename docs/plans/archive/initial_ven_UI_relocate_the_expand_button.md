the 'expand to 24h planning horizon' is taking a lot of UI space. move it below the 'pin cell to top' button so the other controls getting more space. in addition, some asset cells are missing the expand control. why is that? can we add the control to all including its proper functionality?

a) can you confirm? if not, stop the execution and explain
b) make a ui test to assert that the control is now on the right side of the cell, under the pin control.
c) run the test and confirm that it fails (because nothing was fixed yet)
d) fix the now line position in the accumulated power cell diagram
e) run the test and confirm that it is green
f) run all the VEN ui test to check regression
g) if any tests fail (even pre-existing ones, fix them)
h) commit
i) deploy to pi4