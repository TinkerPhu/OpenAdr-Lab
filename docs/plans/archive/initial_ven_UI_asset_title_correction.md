in the asset cells of ven ui on the controller v2 panel, in the right section, the top line (which is in bold as for a title) is still showing the asset power instead of the assets name.
a) can you confirm? if you can not confirm, stop the further execution and explain.
b) create a ui test that asserts that the top line contains the name of the asset and the second line contains the power, the third line contains the price rate, the forth line contains the GHG rate.
c) run the ui test and verify it is failing.
d) fix the issue adding the name of the asset as the first line and move the power one line deeper.
e) run the ui tests again and verify that it is correct. if it is not verified, explain why and suggest how to fix it.